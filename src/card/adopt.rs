//! Taking over a card that already holds music copied by hand. Files a write didn't put there
//! that are exact copies of library songs are moved into the write's layout instead of being
//! copied again (a rename on the card is instant); everything else is left alone, or gathered
//! into one folder if the user asks. Nothing is deleted unless the user also asks to delete
//! clutter (iTunes data, system files, rip logs); the moves can be undone.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use lofty::prelude::*;

use super::copy::Written;
use super::plan::{PlanItem, Target};
use crate::library::Track;
use crate::library::names::{artist_key, clean_title, parse_file_stem, primary_artist, title_key};

/// Extensions treated as music when looking at a card.
const AUDIO: &[&str] = &["flac", "mp3", "wav", "m4a", "m4b", "mp4", "aac", "aiff", "aif", "ogg", "oga", "opus", "ape", "wv", "wma", "mka", "caf"];
/// Bytes compared at each end of two files of the same size to call them copies.
const SAMPLE: u64 = 64 * 1024;

/// The folder a take-over gathers everything else into, when asked to.
pub fn gather_folder() -> String {
    format!("_Before {}", crate::APP_NAME)
}

/// Whether a name at the top of a card is a take-over's gather folder, now or under the app's
/// former name.
fn is_gather_folder(name: &str) -> bool {
    name == gather_folder() || name == format!("_Before {}", crate::FORMER_NAME)
}

/// What's on a card besides what this write put there.
#[derive(Default)]
pub struct Survey {
    /// Exact copies of songs this write puts on the card: (where it is, where the write wants
    /// it, size). Both relative to the card.
    pub used: Vec<(PathBuf, PathBuf, u64)>,
    /// Exact copies of library songs this write doesn't put there as they are (left out,
    /// another edition, or converted for the card).
    pub spare: Vec<(PathBuf, u64)>,
    /// Other versions of library songs (same artist and title, a different file: an MP3 where
    /// the library has FLAC, different tags…). The write puts the library's version on the card.
    pub versions: Vec<(PathBuf, u64)>,
    /// Music that isn't in the library at all.
    pub unknown: Vec<(PathBuf, u64)>,
    /// Everything else: covers, logs, playlists, other devices' data…
    pub other: Vec<(PathBuf, u64)>,
}

impl Survey {
    pub fn bytes(list: &[(PathBuf, u64)]) -> u64 {
        list.iter().map(|(_, n)| n).sum()
    }

    /// Of `other`, the files that are clutter on a music player, by kind: (kind, files, bytes).
    pub fn clutter(&self) -> Vec<(&'static str, usize, u64)> {
        let mut by_kind: Vec<(&'static str, usize, u64)> = Vec::new();
        for (rel, n) in &self.other {
            let Some(kind) = clutter_kind(rel) else { continue };
            match by_kind.iter_mut().find(|(k, _, _)| *k == kind) {
                Some(e) => (e.1, e.2) = (e.1 + 1, e.2 + n),
                None => by_kind.push((kind, 1, *n)),
            }
        }
        by_kind
    }

    pub fn used_bytes(&self) -> u64 {
        self.used.iter().map(|(_, _, n)| n).sum()
    }
}

/// Whether a non-music file on a card is clutter on a music player, and of what kind: data a
/// computer program left behind, which the player can't use and the library doesn't need.
/// Covers, scans, booklets, playlists, CUE sheets and text files are never clutter.
pub fn clutter_kind(rel: &Path) -> Option<&'static str> {
    let name = rel.file_name()?.to_string_lossy().to_lowercase();
    let ext = rel.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let in_itunes = rel.components().next().is_some_and(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case("itunes"));
    if ["itc", "itc2", "itl", "itdb", "ipa", "itlp"].contains(&ext.as_str()) || (in_itunes && ["xml", "plist", "dat", ""].contains(&ext.as_str())) {
        Some("iTunes library data")
    } else if system_file(&name, &ext) {
        Some("Windows and macOS system files")
    } else if ["log", "accurip", "sfv", "nfo", "md5", "ffp"].contains(&ext.as_str()) {
        Some("rip logs and checksums")
    } else {
        None
    }
}

/// Windows' and macOS's folder files, also as backup tools rename them
/// ("desktop (2015_05_12 05_18_04 UTC).ini", "Thumbs (…).db").
fn system_file(name: &str, ext: &str) -> bool {
    let starts = |p: &str| name.starts_with(p) && name[p.len()..].chars().next().is_none_or(|c| c == '.' || c == ' ');
    (ext == "ini" && starts("desktop")) || (ext == "db" && (starts("thumbs") || starts("ehthumbs"))) || name == ".ds_store" || ["tmp", "lnk"].contains(&ext)
}

/// Files on the card this write didn't create (by its manifest), relative to the card, with
/// sizes. Hidden files, system folders, an earlier gather folder and the test card's files are
/// not looked at.
pub fn foreign_files(dest: &Path, manifest: &[Written]) -> Vec<(PathBuf, u64)> {
    let ours: HashSet<String> = manifest.iter().map(|w| w.rel.to_string_lossy().to_lowercase()).collect();
    WalkDir::new(dest)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !n.starts_with('.') && !(e.depth() == 1 && (n == "System Volume Information" || n == "$RECYCLE.BIN" || is_gather_folder(&n) || super::testcard::is_test_name(&n)))
        })
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(dest).ok()?.to_path_buf();
            let size = e.metadata().ok()?.len();
            (!ours.contains(&rel.to_string_lossy().to_lowercase())).then_some((rel, size))
        })
        .collect()
}

/// Sort the card's foreign files into used / spare / other. A card file is a copy of a library
/// song when the sizes match and so do the first and last 64 KB; that reads a little of each
/// candidate, so call it off the UI thread. `progress` gets (done, total).
pub fn survey(tracks: &[Track], plan: &[PlanItem], manifest: &[Written], dest: &Path, mut progress: impl FnMut(usize, usize)) -> Survey {
    let foreign = foreign_files(dest, manifest);
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, t) in tracks.iter().enumerate().filter(|(_, t)| t.cue.is_none()) {
        by_size.entry(t.size).or_default().push(i);
    }
    // Where the write wants each song it copies as it is.
    let mut wanted: HashMap<usize, PathBuf> = plan.iter().filter(|p| p.skip.is_none() && p.target == Target::Copy).map(|p| (p.track, p.rel.clone())).collect();
    let mut samples: HashMap<usize, Option<Vec<u8>>> = HashMap::new();
    // Library songs by artist and title, for recognising other versions of them.
    let songs: HashSet<(String, String)> = tracks.iter().filter_map(song_identity).collect();
    let mut s = Survey::default();
    let total = foreign.len();
    for (n, (rel, size)) in foreign.into_iter().enumerate() {
        progress(n, total);
        let ext = rel.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !AUDIO.contains(&ext.as_str()) {
            s.other.push((rel, size));
            continue;
        }
        // Not a copy: another version of a library song, or music the library doesn't have.
        let elsewhere = |s: &mut Survey, rel: PathBuf| {
            let known = card_song_identity(dest, &rel).is_some_and(|id| songs.contains(&id));
            if known { s.versions.push((rel, size)) } else { s.unknown.push((rel, size)) }
        };
        let Some(candidates) = by_size.get(&size) else {
            elsewhere(&mut s, rel);
            continue;
        };
        let Ok(card_sample) = sample(&dest.join(&rel), size) else {
            s.other.push((rel, size));
            continue;
        };
        let copies: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|&i| samples.entry(i).or_insert_with(|| sample(&tracks[i].path, size).ok()).as_deref() == Some(card_sample.as_slice()))
            .collect();
        if copies.is_empty() {
            elsewhere(&mut s, rel);
            continue;
        }
        // Several identical library files (the same song kept twice): any the write still needs.
        match copies.iter().find_map(|i| wanted.remove(i)) {
            Some(to) => s.used.push((rel, to, size)),
            None => s.spare.push((rel, size)),
        }
    }
    s
}

/// A song as (artist key, title key), for telling two versions of it apart from different songs.
fn song_identity(t: &Track) -> Option<(String, String)> {
    let artist = t.artist.as_deref().or(t.album_artist.as_deref())?;
    let title = t.title.as_deref()?;
    Some(identity(artist, title))
}

fn identity(artist: &str, title: &str) -> (String, String) {
    let artist = primary_artist(artist);
    (artist_key(&artist), title_key(&clean_title(title, &[&artist])))
}

/// The artist and title of a music file on the card: from its tags, else its name
/// ("01 - Artist - Title"). Reads only the file's header.
fn card_song_identity(dest: &Path, rel: &Path) -> Option<(String, String)> {
    let path = dest.join(rel);
    let tagged = lofty::read_from_path(&path).ok().and_then(|f| {
        let tag = f.primary_tag().or_else(|| f.first_tag())?;
        let artist = tag.artist().or_else(|| tag.get_string(ItemKey::AlbumArtist).map(Into::into))?.into_owned();
        Some((artist, tag.title()?.into_owned()))
    });
    let (artist, title) = tagged.or_else(|| {
        let p = parse_file_stem(&path.file_stem()?.to_string_lossy());
        Some((p.artist?, p.title?))
    })?;
    Some(identity(&artist, &title))
}

/// The first and last `SAMPLE` bytes of a file.
pub(crate) fn sample(path: &Path, size: u64) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let head = SAMPLE.min(size);
    let mut out = vec![0; head as usize];
    f.read_exact(&mut out)?;
    if size > SAMPLE {
        let tail = SAMPLE.min(size - SAMPLE);
        f.seek(SeekFrom::End(-(tail as i64)))?;
        let mut t = vec![0; tail as usize];
        f.read_exact(&mut t)?;
        out.extend(t);
    }
    Ok(out)
}

#[derive(Default)]
pub struct Outcome {
    /// Files now in the write's layout: add to its manifest.
    pub written: Vec<Written>,
    pub moved: usize,
    pub gathered: usize,
    /// Clutter files deleted, and their size.
    pub deleted: usize,
    pub deleted_bytes: u64,
    /// macOS "._" and ".DS_Store" files deleted afterwards.
    pub cleaned: usize,
    pub errors: Vec<String>,
}

/// Carry out a survey: optionally delete clutter, then optionally move spare copies and other
/// files into the gather folder (first, so they never stand in the way), then move the used
/// copies into place. Folders left empty are removed, and with `clean_mac` so are the "._" files
/// macOS leaves beside everything it touches on a FAT/exFAT card. Every move is journaled so
/// `undo_latest` can put it back; deleted clutter can't be. `progress` gets (done, total).
pub fn take_over(survey: &Survey, dest: &Path, gather: bool, delete_clutter: bool, clean_mac: bool, write: &str, mut progress: impl FnMut(usize, usize)) -> Outcome {
    let mut out = Outcome::default();
    let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut emptied: HashSet<PathBuf> = HashSet::new();
    let mut deleted: HashSet<&Path> = HashSet::new();
    let clutter: Vec<&(PathBuf, u64)> = if delete_clutter { survey.other.iter().filter(|(rel, _)| clutter_kind(rel).is_some()).collect() } else { Vec::new() };
    let gathered = if gather { survey.spare.len() + survey.versions.len() + survey.unknown.len() + survey.other.len() } else { 0 };
    let total = clutter.len() + gathered + survey.used.len();
    let mut done = 0;
    for (rel, n) in clutter {
        progress(done, total);
        done += 1;
        let path = dest.join(rel);
        match unlocked(&path, || fs::remove_file(&path)) {
            Ok(()) => {
                (out.deleted, out.deleted_bytes) = (out.deleted + 1, out.deleted_bytes + n);
                deleted.insert(rel);
                if let Some(p) = path.parent() {
                    emptied.insert(p.to_path_buf());
                }
            }
            Err(e) => out.errors.push(format!("deleting {}: {}", rel.display(), why(&e))),
        }
    }
    let mut mv = |from: &Path, to: &Path, out: &mut Outcome| -> bool {
        let (a, b) = (dest.join(from), dest.join(to));
        let same = a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase();
        if same && a == b {
            return true;
        }
        // A case-only rename reports the destination as existing on case-insensitive cards.
        if b.exists() && !same {
            out.errors.push(format!("moving {}: {} is already taken", from.display(), to.display()));
            return false;
        }
        let res = b.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| unlocked(&a, || fs::rename(&a, &b)));
        match res {
            Ok(()) => {
                moves.push((from.to_path_buf(), to.to_path_buf()));
                if let Some(p) = a.parent() {
                    emptied.insert(p.to_path_buf());
                }
                true
            }
            Err(e) => {
                out.errors.push(format!("moving {}: {}", from.display(), why(&e)));
                // Don't leave behind the folders made for it.
                if let Some(p) = b.parent() {
                    emptied.insert(p.to_path_buf());
                }
                false
            }
        }
    };
    if gather {
        let folder = PathBuf::from(gather_folder());
        for (rel, _) in survey.spare.iter().chain(&survey.versions).chain(&survey.unknown).chain(&survey.other) {
            progress(done, total);
            done += 1;
            if !deleted.contains(rel.as_path()) && mv(rel, &folder.join(rel), &mut out) {
                out.gathered += 1;
            }
        }
    }
    for (from, to, size) in &survey.used {
        progress(done, total);
        done += 1;
        if mv(from, to, &mut out) {
            out.moved += 1;
            out.written.push(Written { rel: to.clone(), size: *size, source: None });
        }
    }
    progress(total, total);
    remove_emptied(dest, emptied);
    if clean_mac {
        out.cleaned = super::copy::clean_mac_files(dest);
        super::copy::remove_empty_dirs(dest);
    }
    if !moves.is_empty()
        && let Err(e) = save(write, &moves)
    {
        out.errors.push(format!("saving the undo record: {e:#}"));
    }
    out
}

/// Run a file operation, and if it's refused because the file is locked, unlock it and try once
/// more. macOS shows a FAT/exFAT file's read-only attribute as the "locked" flag, and then
/// refuses to rename or delete it; the Echo Mini doesn't care either way.
fn unlocked<T>(path: &Path, op: impl Fn() -> std::io::Result<T>) -> std::io::Result<T> {
    match op() {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && unlock(path) => op(),
        r => r,
    }
}

/// Clear a file's "locked" flag. False if it wasn't locked or couldn't be unlocked.
#[cfg(target_os = "macos")]
fn unlock(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;
    use std::os::unix::ffi::OsStrExt;
    let Ok(flags) = fs::symlink_metadata(path).map(|m| m.st_flags()) else { return false };
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    if flags & libc::UF_IMMUTABLE == 0 {
        return false;
    }
    // SAFETY: `c` is a valid NUL-terminated path; the call only reads it.
    unsafe { libc::chflags(c.as_ptr(), (flags & !libc::UF_IMMUTABLE) as _) == 0 }
}

#[cfg(not(target_os = "macos"))]
fn unlock(_: &Path) -> bool {
    false
}

/// An I/O error in words a person can act on.
fn why(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => "the card refused (the file or its folder is locked or read-only)".into(),
        std::io::ErrorKind::NotFound => "it's no longer there".into(),
        _ => e.to_string(),
    }
}

/// Remove folders that moving files out of left empty (or holding only macOS clutter), and
/// their parents up to the card root. Folders that were empty before are left alone.
fn remove_emptied(dest: &Path, dirs: HashSet<PathBuf>) {
    let mut dirs: Vec<PathBuf> = dirs.into_iter().collect();
    // Deepest first.
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let mut dir = Some(d.as_path());
        while let Some(x) = dir.filter(|x| *x != dest && x.starts_with(dest)) {
            let only_junk = fs::read_dir(x).is_ok_and(|entries| {
                entries.flatten().all(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n == ".DS_Store" || n.starts_with("._")
                })
            });
            if !only_junk || fs::remove_dir_all(x).is_err() {
                break;
            }
            dir = x.parent();
        }
    }
}

/// Where music copied from a card into the music folder goes.
pub const IMPORT_FOLDER: &str = "From the card";

#[derive(Default)]
pub struct Imported {
    pub copied: usize,
    pub bytes: u64,
    /// Already there with the same size (an earlier import).
    pub present: usize,
    pub errors: Vec<String>,
    /// Stopped by the user before every file was copied.
    pub stopped: bool,
}

/// Where music added from another folder (Downloads…) goes in the music folder.
pub const ADDED_FOLDER: &str = "Added";

/// Copy music in `dest` (a card, or a folder of new music) that isn't in the library
/// (`survey.unknown`) into `<music>/<into>/`, keeping its folder names, with the covers, scans,
/// CUE sheets and booklets from the same folders. `dest` isn't changed. `progress` gets
/// (done, total). Once `stop` is set, it stops after the file it's copying.
pub fn import(survey: &Survey, dest: &Path, music: &Path, into: &str, stop: &std::sync::atomic::AtomicBool, mut progress: impl FnMut(usize, usize)) -> Imported {
    const COMPANIONS: &[&str] = &["jpg", "jpeg", "png", "cue", "pdf", "txt", "lrc"];
    let dirs: HashSet<&Path> = survey.unknown.iter().filter_map(|(rel, _)| rel.parent()).collect();
    let companions = survey.other.iter().filter(|(rel, _)| {
        let ext = rel.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        rel.parent().is_some_and(|d| dirs.contains(d)) && COMPANIONS.contains(&ext.as_str())
    });
    let files: Vec<&(PathBuf, u64)> = survey.unknown.iter().chain(companions).collect();
    let root = music.join(into);
    let mut out = Imported::default();
    for (n, (rel, size)) in files.iter().enumerate() {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            out.stopped = true;
            break;
        }
        progress(n, files.len());
        let to = root.join(rel);
        if fs::metadata(&to).is_ok_and(|m| m.len() == *size) {
            out.present += 1;
            continue;
        }
        let res = to.parent().map_or(Ok(()), fs::create_dir_all).map_err(anyhow::Error::from).and_then(|()| super::copy::plain_copy(&dest.join(rel), &to));
        match res {
            Ok(()) => (out.copied, out.bytes) = (out.copied + 1, out.bytes + size),
            Err(e) => out.errors.push(format!("{}: {e:#}", rel.display())),
        }
    }
    out
}

/// The record of one take-over, so it can be undone: card moves, oldest first.
#[derive(Serialize, Deserialize)]
struct Record {
    /// The setup's id; in records made before setups had ids, its name.
    write: String,
    /// (from, to), relative to the card when `relative`, so the record still works with the card
    /// mounted somewhere else; full paths in older records.
    moves: Vec<(PathBuf, PathBuf)>,
    #[serde(default)]
    relative: bool,
}

fn record_dir() -> PathBuf {
    super::journal::journal_dir().join("card")
}

fn save(write: &str, moves: &[(PathBuf, PathBuf)]) -> Result<()> {
    let dir = record_dir();
    fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let rec = Record { write: write.to_string(), moves: moves.to_vec(), relative: true };
    fs::write(dir.join(format!("{stamp}.json")), serde_json::to_vec_pretty(&rec)?)?;
    Ok(())
}

/// The latest take-over by this setup that hasn't been undone, by its id (or, for older
/// records, its name).
pub fn latest(id: &str, name: &str) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(record_dir()).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    files.into_iter().rev().find(|p| fs::read(p).ok().and_then(|b| serde_json::from_slice::<Record>(&b).ok()).is_some_and(|r| r.write == id || r.write == name))
}

/// Put back the latest take-over of this write. Returns the card paths (relative to `dest`) that
/// are no longer the write's (remove them from its manifest), how many files went back, and errors.
pub fn undo_latest(id: &str, name: &str, dest: &Path) -> Result<(Vec<PathBuf>, usize, Vec<String>)> {
    let path = latest(id, name).context("nothing to undo")?;
    let rec: Record = serde_json::from_slice(&fs::read(&path)?)?;
    let (mut released, mut restored, mut errors) = (Vec::new(), 0, Vec::new());
    let mut emptied = HashSet::new();
    let on_card = |p: &PathBuf| if rec.relative { dest.join(p) } else { p.clone() };
    for (old, new) in rec.moves.iter().rev() {
        let (old, new) = (&on_card(old), &on_card(new));
        let res = (|| -> Result<()> {
            if let Some(p) = old.parent() {
                fs::create_dir_all(p)?;
            }
            unlocked(new, || super::journal::move_back(new, old))?;
            Ok(())
        })();
        match res {
            Ok(()) => {
                restored += 1;
                if let Ok(rel) = new.strip_prefix(dest) {
                    released.push(rel.to_path_buf());
                }
                if let Some(p) = new.parent() {
                    emptied.insert(p.to_path_buf());
                }
            }
            Err(e) => errors.push(format!("{}: {e:#}", new.display())),
        }
    }
    remove_emptied(dest, emptied);
    fs::rename(&path, path.with_extension("undone"))?;
    Ok((released, restored, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PAUSETAPE_SURVEY=<card>:<music folder> cargo test -- --ignored survey_a_real_card --nocapture`
    /// Reads only: what a default write would take over on that card.
    #[test]
    #[ignore]
    fn survey_a_real_card() {
        let Ok(spec) = std::env::var("PAUSETAPE_SURVEY") else { return };
        let (card, music) = spec.split_once(':').unwrap();
        let mut db = crate::library::db::Db::open_default().ok();
        let (tracks, _) = crate::library::scan(Path::new(music), db.as_mut(), false, |_| {});
        let index = crate::library::grouping::build(&tracks, &Default::default());
        let w = crate::card::WriteProfile::default();
        let plan = w.plan(&tracks, &index);
        let t = std::time::Instant::now();
        let s = survey(&tracks, &plan, &[], Path::new(card), |_, _| {});
        let gb = |n: u64| n as f64 / 1e9;
        println!("surveyed in {:.1?}", t.elapsed());
        println!("used:  {} files, {:.1} GB", s.used.len(), gb(s.used_bytes()));
        println!("spare: {} files, {:.1} GB", s.spare.len(), gb(Survey::bytes(&s.spare)));
        println!("versions: {} files, {:.1} GB", s.versions.len(), gb(Survey::bytes(&s.versions)));
        println!("unknown: {} files, {:.1} GB", s.unknown.len(), gb(Survey::bytes(&s.unknown)));
        println!("other: {} files, {:.1} GB", s.other.len(), gb(Survey::bytes(&s.other)));
        for (kind, n, b) in s.clutter() {
            println!("  clutter, {kind}: {n} files, {:.2} GB", gb(b));
        }
        let mut albums: std::collections::BTreeMap<String, usize> = Default::default();
        for (rel, _) in &s.unknown {
            *albums.entry(rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default()).or_default() += 1;
        }
        println!("unknown, by top folder: {:?}", albums.iter().take(25).collect::<Vec<_>>());
        let mut by_ext: std::collections::BTreeMap<String, (usize, u64)> = Default::default();
        for (rel, n) in &s.other {
            let e = by_ext.entry(rel.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()).or_default();
            (e.0, e.1) = (e.0 + 1, e.1 + n);
        }
        for (ext, (n, b)) in by_ext.iter().filter(|(_, (_, b))| *b > 50_000_000) {
            println!("  other .{ext}: {n} files, {:.1} GB", gb(*b));
        }
        for (from, to, _) in s.used.iter().take(4) {
            println!("  {} → {}", from.display(), to.display());
        }
    }
    use crate::card::{Custom, Options, build_plan};
    use crate::library::grouping::{ArtistPrefs, build};
    use crate::library::scan;

    #[test]
    fn music_only_on_the_card_is_copied_into_the_library() {
        let root = std::env::temp_dir().join(format!("pausetape-test-import-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (music, card) = (root.join("music"), root.join("card"));
        for (p, body) in [
            ("music/Muse/2003 - Absolution/01 - Intro.mp3", "intro"),
            ("card/Dead Can Dance - Aion (1990)/01 - The Arrival.flac", "arrival"),
            ("card/Dead Can Dance - Aion (1990)/folder.jpg", "jpeg"),
            ("card/Dead Can Dance - Aion (1990)/desktop.ini", "ini"),
            ("card/Muse copy/01 - Intro.mp3", "intro"),
        ] {
            let p = root.join(p);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        let (tracks, _) = crate::library::scan(&music, None, false, |_| {});
        let s = survey(&tracks, &[], &[], &card, |_, _| {});
        assert_eq!(s.unknown.len(), 1);
        let out = import(&s, &card, &music, IMPORT_FOLDER, &Default::default(), |_, _| {});
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(out.copied, 2, "the song and its cover; not the system file, not the library's own song");
        let into = music.join(IMPORT_FOLDER).join("Dead Can Dance - Aion (1990)");
        assert!(into.join("01 - The Arrival.flac").exists() && into.join("folder.jpg").exists() && !into.join("desktop.ini").exists());
        assert!(card.join("Dead Can Dance - Aion (1990)/01 - The Arrival.flac").exists(), "the card is unchanged");
        assert_eq!(import(&s, &card, &music, IMPORT_FOLDER, &Default::default(), |_, _| {}).present, 2, "a second import copies nothing");

        // Once in the library, the song is an exact copy a write can take over.
        let (tracks, _) = crate::library::scan(&music, None, false, |_| {});
        let s = survey(&tracks, &[], &[], &card, |_, _| {});
        assert!(s.unknown.is_empty() && s.spare.len() == 2, "{} unknown, {} spare", s.unknown.len(), s.spare.len());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_hand_copied_card_is_taken_over_and_can_be_undone() {
        let root = std::env::temp_dir().join(format!("pausetape-test-adopt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
        let song = |p: &Path, body: &[u8]| {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        };
        // The library, and the same songs copied by hand into differently named folders.
        song(&src.join("Muse/2003 - Absolution/01 - Intro.mp3"), b"intro audio");
        song(&src.join("Muse/2003 - Absolution/02 - Apocalypse Please.mp3"), b"apocalypse audio");
        song(&src.join("Muse/2001 - Origin/01 - New Born.m4a"), b"aac audio");
        song(&card.join("muse absolution [mp3]/01 Intro.mp3"), b"intro audio");
        song(&card.join("muse absolution [mp3]/02 Apocalypse.mp3"), b"apocalypse audio");
        song(&card.join("muse absolution [mp3]/cover.jpg"), b"jpeg");
        song(&card.join("Origin/01.m4a"), b"aac audio");
        song(&card.join("Someone Else/song.mp3"), b"not in the library");
        song(&card.join("Mixed/Muse - New Born.mp3"), b"an mp3 of a song the library has as m4a");
        song(&card.join("muse absolution [mp3]/desktop.ini"), b"[.ShellClassInfo]");
        song(&card.join("muse absolution [mp3]/rip.log"), b"EAC");
        song(&card.join("iTunes/Album Artwork/Cache/abc.itc2"), b"art cache");
        song(&card.join("iTunes/iTunes Library.itl"), b"library");
        // What macOS leaves beside files on a FAT/exFAT card.
        song(&card.join("muse absolution [mp3]/._01 Intro.mp3"), b"AppleDouble");

        let (tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let opts = Options { unplayable: crate::card::Unplayable::Leave, ..Default::default() };
        let plan = build_plan(&tracks, &index, &opts, &Custom::default());
        let mut s = survey(&tracks, &plan, &[], &card, |_, _| {});
        assert_eq!(s.used.len(), 2, "both Absolution songs are exact copies the write wants");
        assert_eq!(s.spare.len(), 1, "the M4A is a copy of a song this write leaves out");
        assert_eq!((s.unknown.len(), s.other.len()), (1, 5), "the unknown song; the cover and four files of clutter");
        let kinds: Vec<(&str, usize)> = s.clutter().iter().map(|(k, n, _)| (*k, *n)).collect();
        assert!(kinds.contains(&("iTunes library data", 2)) && kinds.contains(&("Windows and macOS system files", 1)) && kinds.contains(&("rip logs and checksums", 1)), "{kinds:?}");
        assert_eq!(clutter_kind(Path::new("Album/cover.jpg")), None);
        assert!(clutter_kind(Path::new("Album/desktop (2015_05_12 05_18_04 UTC).ini")).is_some());
        assert!(clutter_kind(Path::new("Album/Thumbs (2015_05_12 05_18_04 UTC).db")).is_some());
        assert_eq!(clutter_kind(Path::new("Album/settings.ini")), None);
        assert_eq!(clutter_kind(Path::new("Album/Album.cue")), None);
        assert_eq!(s.versions.len(), 1, "another version of a library song");

        // A locked file (a FAT read-only attribute, as macOS shows it) is unlocked and moved.
        #[cfg(target_os = "macos")]
        assert!(std::process::Command::new("chflags").arg("uchg").arg(card.join("muse absolution [mp3]/02 Apocalypse.mp3")).status().unwrap().success());
        // A file that went away since the survey: reported, and no folder is left for it.
        s.unknown.push((PathBuf::from("Vanished/song.mp3"), 1));
        let out = take_over(&s, &card, true, true, true, "test write", |_, _| {});
        assert_eq!(out.errors.len(), 1, "{:?}", out.errors);
        assert!(out.errors[0].contains("Vanished/song.mp3") && out.errors[0].contains("no longer there"), "{:?}", out.errors);
        assert!(!card.join("_Before Pausetape/Vanished").exists());
        assert_eq!((out.moved, out.gathered, out.written.len(), out.deleted), (2, 4, 2, 4));
        assert!(WalkDir::new(&card).into_iter().flatten().all(|e| !e.file_name().to_string_lossy().starts_with("._")), "macOS files cleaned");
        assert!(!card.join("iTunes").exists(), "clutter deleted, its folders removed");
        assert!(card.join("Muse/2003 - Absolution/01 - Intro.mp3").exists());
        assert!(card.join("_Before Pausetape/Someone Else/song.mp3").exists());
        assert!(!card.join("muse absolution [mp3]").exists(), "emptied folders go");

        // Found by the setup's id; records from before ids hold its name, so that works too.
        assert!(latest("test write", "renamed since").is_some() && latest("another id", "test write").is_some());
        assert!(latest("another id", "another name").is_none());
        // The card comes back mounted somewhere else ("Untitled 1"): undo still finds its files.
        let moved = root.join("card 1");
        fs::rename(&card, &moved).unwrap();
        let (released, restored, errors) = undo_latest("test write", "", &moved).unwrap();
        fs::rename(&moved, &card).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!((released.len(), restored), (6, 6));
        assert!(card.join("muse absolution [mp3]/02 Apocalypse.mp3").exists());
        assert!(card.join("Someone Else/song.mp3").exists());
        assert!(!card.join("Muse").exists() && !card.join("_Before Pausetape").exists());
        assert!(!card.join("muse absolution [mp3]/rip.log").exists(), "deleted clutter doesn't come back");
        fs::remove_dir_all(root).unwrap();
    }
}
