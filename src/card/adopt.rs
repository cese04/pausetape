//! Taking over a card that already holds music copied by hand. Files a write didn't put there
//! that are exact copies of library songs are moved into the write's layout instead of being
//! copied again (a rename on the card is instant); everything else is left alone, or gathered
//! into one folder if the user asks. Nothing is deleted, and the moves can be undone.

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

    pub fn used_bytes(&self) -> u64 {
        self.used.iter().map(|(_, _, n)| n).sum()
    }
}

/// Files on the card this write didn't create (by its manifest), relative to the card, with
/// sizes. Hidden files, system folders and an earlier gather folder are not looked at.
pub fn foreign_files(dest: &Path, manifest: &[Written]) -> Vec<(PathBuf, u64)> {
    let ours: HashSet<String> = manifest.iter().map(|w| w.rel.to_string_lossy().to_lowercase()).collect();
    let gather = gather_folder();
    WalkDir::new(dest)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !n.starts_with('.') && !(e.depth() == 1 && (n == "System Volume Information" || n == "$RECYCLE.BIN" || n == gather.as_str()))
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
fn sample(path: &Path, size: u64) -> std::io::Result<Vec<u8>> {
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
    pub errors: Vec<String>,
}

/// Carry out a survey: optionally move spare copies and other files into the gather folder
/// (first, so they never stand in the way), then move the used copies into place. Folders left
/// empty are removed. Every move is journaled so `undo_latest` can put it back.
pub fn take_over(survey: &Survey, dest: &Path, gather: bool, write_name: &str) -> Outcome {
    let mut out = Outcome::default();
    let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut emptied: HashSet<PathBuf> = HashSet::new();
    let mut mv = |from: &Path, to: &Path, out: &mut Outcome| -> bool {
        let (a, b) = (dest.join(from), dest.join(to));
        let same = a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase();
        let res = (|| -> Result<()> {
            if same && a == b {
                return Ok(());
            }
            // A case-only rename reports the destination as existing on case-insensitive cards.
            if b.exists() && !same {
                anyhow::bail!("{} is already taken", to.display());
            }
            if let Some(p) = b.parent() {
                fs::create_dir_all(p)?;
            }
            fs::rename(&a, &b).with_context(|| format!("moving {}", from.display()))
        })();
        match res {
            Ok(()) => {
                if a != b {
                    moves.push((a.clone(), b));
                    if let Some(p) = a.parent() {
                        emptied.insert(p.to_path_buf());
                    }
                }
                true
            }
            Err(e) => {
                out.errors.push(format!("{e:#}"));
                false
            }
        }
    };
    if gather {
        let folder = PathBuf::from(gather_folder());
        for (rel, _) in survey.spare.iter().chain(&survey.versions).chain(&survey.unknown).chain(&survey.other) {
            if mv(rel, &folder.join(rel), &mut out) {
                out.gathered += 1;
            }
        }
    }
    for (from, to, size) in &survey.used {
        if mv(from, to, &mut out) {
            out.moved += 1;
            out.written.push(Written { rel: to.clone(), size: *size, source: None });
        }
    }
    remove_emptied(dest, emptied);
    if !moves.is_empty()
        && let Err(e) = save(write_name, &moves)
    {
        out.errors.push(format!("saving the undo record: {e:#}"));
    }
    out
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

/// The record of one take-over, so it can be undone: card moves, oldest first.
#[derive(Serialize, Deserialize)]
struct Record {
    write: String,
    moves: Vec<(PathBuf, PathBuf)>,
}

fn record_dir() -> PathBuf {
    super::journal::journal_dir().join("card")
}

fn save(write_name: &str, moves: &[(PathBuf, PathBuf)]) -> Result<()> {
    let dir = record_dir();
    fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let rec = Record { write: write_name.to_string(), moves: moves.to_vec() };
    fs::write(dir.join(format!("{stamp}.json")), serde_json::to_vec_pretty(&rec)?)?;
    Ok(())
}

/// The latest take-over of this write that hasn't been undone.
pub fn latest(write_name: &str) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(record_dir()).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    files.into_iter().rev().find(|p| fs::read(p).ok().and_then(|b| serde_json::from_slice::<Record>(&b).ok()).is_some_and(|r| r.write == write_name))
}

/// Put back the latest take-over of this write. Returns the card paths (relative to `dest`) that
/// are no longer the write's (remove them from its manifest), how many files went back, and errors.
pub fn undo_latest(write_name: &str, dest: &Path) -> Result<(Vec<PathBuf>, usize, Vec<String>)> {
    let path = latest(write_name).context("nothing to undo")?;
    let rec: Record = serde_json::from_slice(&fs::read(&path)?)?;
    let (mut released, mut restored, mut errors) = (Vec::new(), 0, Vec::new());
    let mut emptied = HashSet::new();
    for (old, new) in rec.moves.iter().rev() {
        let res = (|| -> Result<()> {
            if let Some(p) = old.parent() {
                fs::create_dir_all(p)?;
            }
            fs::rename(new, old)?;
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

    /// `NEATLIBRARY_SURVEY=<card>:<music folder> cargo test -- --ignored survey_a_real_card --nocapture`
    /// Reads only: what a default write would take over on that card.
    #[test]
    #[ignore]
    fn survey_a_real_card() {
        let Ok(spec) = std::env::var("NEATLIBRARY_SURVEY") else { return };
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
    fn a_hand_copied_card_is_taken_over_and_can_be_undone() {
        let root = std::env::temp_dir().join(format!("neatlibrary-test-adopt-{}", std::process::id()));
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

        let (tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let opts = Options { unplayable: crate::card::Unplayable::Leave, ..Default::default() };
        let plan = build_plan(&tracks, &index, &opts, &Custom::default());
        let s = survey(&tracks, &plan, &[], &card, |_, _| {});
        assert_eq!(s.used.len(), 2, "both Absolution songs are exact copies the write wants");
        assert_eq!(s.spare.len(), 1, "the M4A is a copy of a song this write leaves out");
        assert_eq!((s.unknown.len(), s.other.len()), (1, 1), "the unknown song, and the cover");
        assert_eq!(s.versions.len(), 1, "another version of a library song");

        let out = take_over(&s, &card, true, "test write");
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!((out.moved, out.gathered, out.written.len()), (2, 4, 2));
        assert!(card.join("Muse/2003 - Absolution/01 - Intro.mp3").exists());
        assert!(card.join("_Before NeatLibrary/Someone Else/song.mp3").exists());
        assert!(!card.join("muse absolution [mp3]").exists(), "emptied folders go");

        let (released, restored, errors) = undo_latest("test write", &card).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!((released.len(), restored), (6, 6));
        assert!(card.join("muse absolution [mp3]/02 Apocalypse.mp3").exists());
        assert!(card.join("Someone Else/song.mp3").exists());
        assert!(!card.join("Muse").exists() && !card.join("_Before NeatLibrary").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
