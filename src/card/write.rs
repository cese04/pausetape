//! Saved "writes": named layouts for an SD card, and the sync that applies one.
//!
//! A write remembers what goes on the card (artists, albums, playlists), what to call it, in what
//! order, and a manifest of the files it put there last time. The manifest is what makes it safe
//! to remove deselected music: only files this write created (and that still have the size it
//! recorded) are ever deleted.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::playlist::{self, Playlist};
use super::plan::{Target, card_size};
use super::{Custom, Mode, Options, PlanItem, Progress, Written};
use crate::library::Track;
use crate::library::names::sanitize_component;
use crate::library::grouping::Index;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WriteProfile {
    /// Names the setup on the card's label (`card::label`); unlike the name, never changes.
    pub id: String,
    pub name: String,
    pub dest: Option<PathBuf>,
    /// The folder on the card the music and playlists go in (`MUSIC` for a Walkman), with `/`
    /// between levels; empty for the card's top level, as setups had before there was a choice.
    pub music_folder: String,
    pub options: Options,
    pub custom: Custom,
    /// Names of playlists to put on the card.
    pub playlists: BTreeSet<String>,
    pub playlists_folder: String,
    /// Remove files this write put on the card earlier that are no longer part of it.
    pub prune: bool,
    pub manifest: Vec<Written>,
    /// Unix seconds of the last completed write.
    pub last_written: Option<u64>,
    /// The take-over dialog was shown on its own once for this card; later it opens on request.
    pub take_over_offered: bool,
    /// The id of the card `manifest` describes (`Label::card`). Another card at `dest` is a new one.
    pub card_id: Option<String>,
}

impl Default for WriteProfile {
    fn default() -> Self {
        WriteProfile {
            id: super::label::new_id(),
            name: "Echo Mini".into(),
            dest: None,
            music_folder: String::new(),
            options: Options::default(),
            custom: Custom::default(),
            playlists: BTreeSet::new(),
            playlists_folder: "_Playlists".into(),
            prune: true,
            manifest: Vec::new(),
            last_written: None,
            take_over_offered: false,
            card_id: None,
        }
    }
}

impl WriteProfile {
    pub fn plan(&self, tracks: &[Track], index: &Index) -> Vec<PlanItem> {
        let mut plan = super::build_plan(tracks, index, &self.options, &self.custom);
        // Song folders keep clear of the folders the app keeps for itself on the card (an
        // artist called "_Playlists"): they'd be taken for its playlist copies or test samples.
        let own = [sanitize_component(&self.playlists_folder, 80).to_lowercase(), super::testcard::TEST_FOLDER.to_lowercase()];
        let root = self.music_root();
        for p in &mut plan {
            let mut parts = p.rel.components();
            let Some(first) = parts.next().map(|c| c.as_os_str().to_string_lossy().into_owned()) else { continue };
            if parts.clone().next().is_some() && own.contains(&first.to_lowercase()) {
                p.rel = PathBuf::from(format!("{first}_")).join(parts.as_path());
            }
            p.rel = root.join(&p.rel);
        }
        plan
    }

    /// `music_folder` as a path on the card, each level made safe for FAT; empty for the top.
    pub fn music_root(&self) -> PathBuf {
        // Leading dots go (no hidden folder, no ".."), and a level left with nothing is dropped.
        self.music_folder.split(['/', '\\']).filter(|c| !c.trim().trim_start_matches('.').trim().is_empty()).map(|c| sanitize_component(c, 80)).collect()
    }

    /// Where the playlists' folders go on the card, inside `music_root`.
    pub fn playlists_dir(&self) -> PathBuf {
        self.music_root().join(sanitize_component(&self.playlists_folder, 80))
    }

    pub fn chosen_playlists(&self, all: &[Playlist]) -> Vec<Playlist> {
        all.iter().filter(|p| self.playlists.contains(&p.name)).cloned().collect()
    }

    pub fn manifest_bytes(&self) -> u64 {
        self.manifest.iter().map(|w| w.size).sum()
    }

    /// A card file of this write that's one of its playlist copies: in its playlists folder,
    /// or that folder at the card's top, where it was before the music had a folder.
    fn in_playlists(&self, rel: &Path) -> bool {
        let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
        let rel = lower(rel);
        rel.starts_with(lower(&self.playlists_dir())) || rel.starts_with(sanitize_component(&self.playlists_folder, 80).to_lowercase())
    }
}

/// Songs the last write put on the card that the plan now names differently (another pattern, a
/// renamed album or artist, another order): (manifest index, plan index), to move on the card
/// rather than copy again. Matched by what each was made from: a converted file by its source
/// key, a copy by size and format. Playlist copies are the playlist export's.
pub fn renamed(w: &WriteProfile, tracks: &[Track], plan: &[PlanItem]) -> Vec<(usize, usize)> {
    let lower = |p: &Path| p.to_string_lossy().to_lowercase();
    let ext = |p: &Path| p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let wanted: HashSet<String> = plan.iter().filter(|p| p.skip.is_none()).map(|p| lower(&p.rel)).collect();
    let on_card: HashSet<String> = w.manifest.iter().map(|m| lower(&m.rel)).collect();
    let mut stale: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in w.manifest.iter().enumerate().rev() {
        if !wanted.contains(&lower(&m.rel)) && !w.in_playlists(&m.rel) {
            let key = m.source.clone().unwrap_or_else(|| format!("{}|{}", m.size, ext(&m.rel)));
            stale.entry(key).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for (j, p) in plan.iter().enumerate() {
        let t = &tracks[p.track];
        if p.skip.is_some() || on_card.contains(&lower(&p.rel)) {
            continue;
        }
        let key = super::copy::source_key(t, p.target).unwrap_or_else(|| format!("{}|{}", t.size, ext(&p.rel)));
        if let Some(i) = stale.get_mut(&key).and_then(Vec::pop) {
            out.push((i, j));
        }
    }
    out
}

/// (songs a Sync with `plan` would take off the card, songs the setup has there). Songs that
/// only change name don't count; playlist copies aren't songs here.
pub fn removing(w: &WriteProfile, tracks: &[Track], plan: &[PlanItem]) -> (usize, usize) {
    let wanted: HashSet<String> = plan.iter().filter(|p| p.skip.is_none()).map(|p| p.rel.to_string_lossy().to_lowercase()).collect();
    let moving: HashSet<usize> = renamed(w, tracks, plan).into_iter().map(|(i, _)| i).collect();
    let songs: Vec<(usize, &Written)> = w.manifest.iter().enumerate().filter(|(_, m)| !w.in_playlists(&m.rel)).collect();
    let gone = songs.iter().filter(|(i, m)| !moving.contains(i) && !wanted.contains(&m.rel.to_string_lossy().to_lowercase())).count();
    (gone, songs.len())
}

/// Move what `renamed` finds to its new name. A copy moves only if its first and last 64 KB are
/// the library song's. The manifest with the new names, how many moved, and what went wrong.
fn relocate(w: &WriteProfile, tracks: &[Track], plan: &[PlanItem], dest: &Path) -> (Vec<Written>, usize, Vec<String>) {
    let mut manifest = w.manifest.clone();
    let (mut moved, mut errors) = (0, Vec::new());
    for (i, j) in renamed(w, tracks, plan) {
        let (m, p, t) = (&manifest[i], &plan[j], &tracks[plan[j].track]);
        // Records come from our own manifest, but never follow one outside the card.
        if !m.rel.components().chain(p.rel.components()).all(|c| matches!(c, std::path::Component::Normal(_))) {
            continue;
        }
        let (old, new) = (dest.join(&m.rel), dest.join(&p.rel));
        if !std::fs::metadata(&old).is_ok_and(|md| md.len() == m.size) {
            continue;
        }
        // Names differing only in case are the same file on the card, so never come here.
        // A file made or retagged for the card is known by its key; a plain copy, by its bytes.
        let same = super::copy::source_key(t, p.target).is_some() || matches!((super::adopt::sample(&old, m.size), super::adopt::sample(&t.path, t.size)), (Ok(a), Ok(b)) if a == b);
        if !same || new.exists() {
            continue;
        }
        let res = new.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::rename(&old, &new));
        if let Err(e) = res {
            errors.push(format!("renaming {} on the card: {e}", m.rel.display()));
            continue;
        }
        // macOS's "._" companion stays behind under the old name.
        if let Some(n) = old.file_name() {
            let _ = std::fs::remove_file(old.with_file_name(format!("._{}", n.to_string_lossy())));
        }
        let mut dir = old.parent();
        while let Some(d) = dir.filter(|d| *d != dest && d.starts_with(dest)) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
        manifest[i].rel = p.rel.clone();
        moved += 1;
    }
    (manifest, moved, errors)
}

/// Where the plan puts each song on the card: track id → card path, for playlists.
pub fn organised(tracks: &[Track], plan: &[PlanItem]) -> HashMap<PathBuf, PathBuf> {
    plan.iter().filter(|p| p.skip.is_none()).map(|p| (tracks[p.track].id(), p.rel.clone())).collect()
}

#[derive(Default)]
pub struct Preview {
    pub to_copy: usize,
    /// Of `to_copy`, files ffmpeg makes (converted songs, songs cut from album files).
    pub to_convert: usize,
    pub copy_bytes: u64,
    pub unchanged: usize,
    pub skipped: usize,
    /// Files from the previous write that would be removed.
    pub to_remove: Vec<PathBuf>,
    pub playlist_files: usize,
    /// Songs already on the card under another name: moved there, not copied.
    pub renamed: usize,
    pub total_bytes: u64,
    /// Files on the card this write didn't put there (by its manifest), and their size.
    pub foreign: usize,
    pub foreign_bytes: u64,
    /// An earlier take-over of the card by this write can be undone.
    pub can_undo_take_over: bool,
}

/// What a write would do, comparing the plan with the card. Touches the card read-only.
pub fn preview(w: &WriteProfile, tracks: &[Track], index: &Index, playlists: &[Playlist], dest: &Path) -> Preview {
    let mut p = Preview::default();
    let mut keep: HashSet<String> = HashSet::new();
    let previous = super::copy::by_rel(&w.manifest);
    let check = |rel: &Path, t: &Track, to: Target, p: &mut Preview, keep: &mut HashSet<String>| {
        let key = rel.to_string_lossy().to_lowercase();
        let up_to_date = super::up_to_date(t, to, &dest.join(rel), previous.get(&key).copied());
        keep.insert(key);
        let bytes = card_size(t, to);
        p.total_bytes += bytes;
        if up_to_date {
            p.unchanged += 1;
        } else {
            p.to_copy += 1;
            p.copy_bytes += bytes;
            if to != Target::Copy {
                p.to_convert += 1;
            }
        }
    };
    let plan = w.plan(tracks, index);
    let moves = renamed(w, tracks, &plan);
    let moving: HashSet<usize> = moves.iter().map(|&(_, j)| j).collect();
    let moved_from: HashSet<String> = moves.iter().map(|&(i, _)| w.manifest[i].rel.to_string_lossy().to_lowercase()).collect();
    for (j, item) in plan.iter().enumerate() {
        if item.skip.is_some() {
            p.skipped += 1;
        } else if moving.contains(&j) {
            p.renamed += 1;
            p.total_bytes += card_size(&tracks[item.track], item.target);
            keep.insert(item.rel.to_string_lossy().to_lowercase());
        } else {
            check(&item.rel, &tracks[item.track], item.target, &mut p, &mut keep);
        }
    }
    let ids: Vec<PathBuf> = tracks.iter().map(Track::id).collect();
    let by_id: HashMap<&Path, &Track> = ids.iter().map(PathBuf::as_path).zip(tracks).collect();
    for (dir, copies, _) in playlist::planned(&w.chosen_playlists(playlists), &by_id, &organised(tracks, &plan), &w.playlists_dir(), w.options.rules()) {
        let copies: Vec<_> = copies.into_iter().filter(|c| c.copy).collect();
        p.playlist_files += copies.len();
        for c in copies {
            check(&c.rel, c.track, c.target, &mut p, &mut keep);
        }
        keep.insert(dir.with_file_name(format!("{}.m3u8", dir.file_name().unwrap().to_string_lossy())).to_string_lossy().to_lowercase());
    }
    if w.prune {
        p.to_remove = w.manifest.iter().map(|m| (m, m.rel.to_string_lossy().to_lowercase())).filter(|(_, k)| !keep.contains(k) && !moved_from.contains(k)).map(|(m, _)| m.rel.clone()).collect();
    }
    let foreign = super::adopt::foreign_files(dest, &w.manifest);
    (p.foreign, p.foreign_bytes) = (foreign.len(), foreign.iter().map(|(_, n)| n).sum());
    p.can_undo_take_over = super::adopt::latest(&w.id, &w.name).is_some();
    p
}

#[derive(Default)]
pub struct Outcome {
    pub manifest: Vec<Written>,
    pub copied: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub tagged: usize,
    pub removed: usize,
    /// Songs moved to a new name on the card rather than copied again.
    pub renamed: usize,
    pub cleaned: usize,
    pub errors: Vec<String>,
    /// Stopped by the user before the end: playlists and the last clean-up were left for the next write.
    pub stopped: bool,
    /// The card wasn't there, or went away: the record keeps what it had, plus what got copied.
    pub card_gone: bool,
    /// The card's id, once its label is written.
    pub card_id: Option<String>,
}

/// Remove what the previous write left that the plan no longer wants, copy the plan and
/// playlists to the card, then remove what's left over (old playlist files and the like). A song
/// the plan still wants stays even if its new copy failed: an old copy beats none.
///
/// Once `stop` is set, the song being copied is finished and the write ends there. Its
/// manifest then holds what it copied plus the earlier files still on the card, so the next
/// write carries on from there and can still remove anything of its own. A card that goes away
/// ends the write the same way, keeping the earlier records, as nothing can be checked.
#[allow(clippy::too_many_arguments)]
pub fn write(
    w: &WriteProfile,
    tracks: &[Track],
    plan: &[PlanItem],
    playlists: &[Playlist],
    source: &Path,
    dest: &Path,
    stop: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Outcome {
    if !super::connected(dest) {
        return Outcome { manifest: w.manifest.clone(), card_gone: true, ..Default::default() };
    }
    let drive = super::copy::drive_of(dest);
    // Labelled first, so a write stopped halfway still says whose card it is.
    let label = super::Label::for_card(w, dest);
    let labelled = label.save(dest).map_err(|e| format!("{e:#}"));
    let card_id = labelled.is_ok().then_some(label.card);
    // Songs that only changed name move on the card instead of being copied again.
    let (manifest, renamed, rename_errors) = relocate(w, tracks, plan, dest);
    let moved = WriteProfile { manifest, ..w.clone() };
    let w = &moved;
    // Removing first makes room: a choice that fits only once old albums are gone would
    // otherwise fill the card halfway. Playlist files wait for the end, when their names are known.
    let early = if w.prune {
        let wanted: HashSet<String> = plan.iter().filter(|p| p.skip.is_none()).map(|p| p.rel.to_string_lossy().to_lowercase()).collect();
        let songs: Vec<Written> = w.manifest.iter().filter(|m| !w.in_playlists(&m.rel)).cloned().collect();
        super::prune(dest, &songs, &wanted)
    } else {
        0
    };
    // The card is tidied once at the end, after playlists too (`tidy_card`): halves of files a
    // write cut short left behind (the card pulled out mid-copy), Pausetape's own by their name
    // and no use to anyone, and macOS's files if wanted.
    let tidy = || super::tidy_card(dest, w.options.clean_mac_files).1;
    let opts = Options { clean_mac_files: false, ..w.options.clone() };
    let mut r = super::execute(tracks, plan, source, dest, Mode::CopyTo, &opts, &w.manifest, stop, &mut progress);
    r.errors.extend(labelled.err());
    r.errors.extend(rename_errors);
    if r.stopped || r.card_gone {
        let mut manifest = r.written;
        let mut have: HashSet<String> = manifest.iter().map(|m| m.rel.to_string_lossy().to_lowercase()).collect();
        manifest.extend(w.manifest.iter().filter(|m| have.insert(m.rel.to_string_lossy().to_lowercase()) && (r.card_gone || dest.join(&m.rel).exists())).cloned());
        let cleaned = if r.card_gone { 0 } else { tidy() };
        return Outcome { manifest, copied: r.copied, unchanged: r.unchanged, skipped: r.skipped, tagged: r.tagged, removed: early, renamed, cleaned, errors: r.errors, stopped: r.stopped, card_gone: r.card_gone, card_id };
    }

    let organised = organised(tracks, plan);
    let pl = playlist::export(&w.chosen_playlists(playlists), tracks, &organised, dest, &w.playlists_dir(), w.options.rules(), &w.manifest, |_, _| {});

    let mut manifest = r.written;
    manifest.extend(pl.written);
    let mut keep: HashSet<String> = manifest.iter().map(|m| m.rel.to_string_lossy().to_lowercase()).collect();
    keep.extend(plan.iter().filter(|p| p.skip.is_none()).map(|p| p.rel.to_string_lossy().to_lowercase()));
    let removed = if w.prune { super::prune(dest, &w.manifest, &keep) } else { 0 };
    let card_gone = !super::copy::card_there(dest, drive);
    let cleaned = if card_gone { 0 } else { tidy() };
    // What's recorded is what's on the card: earlier files this write neither replaced nor could
    // remove (a song whose new copy failed, a write-protected card) stay this setup's.
    if w.prune || card_gone {
        let mut have: HashSet<String> = manifest.iter().map(|m| m.rel.to_string_lossy().to_lowercase()).collect();
        let earlier: Vec<Written> = w.manifest.iter().filter(|m| have.insert(m.rel.to_string_lossy().to_lowercase()) && (card_gone || std::fs::metadata(dest.join(&m.rel)).is_ok_and(|md| md.len() == m.size))).cloned().collect();
        manifest.extend(earlier);
    }

    let mut errors = r.errors;
    if card_gone && !pl.errors.is_empty() {
        // Pulled out while the playlists were written: one line says so, rather than the system's
        // answer for each file.
        errors.push("playlists: the card went away while they were written".into());
    } else {
        errors.extend(pl.errors);
    }
    Outcome {
        manifest,
        copied: r.copied + pl.copied,
        unchanged: r.unchanged + pl.unchanged,
        skipped: r.skipped,
        tagged: r.tagged,
        removed: early + removed + pl.removed,
        renamed,
        cleaned,
        errors,
        stopped: false,
        card_gone,
        card_id,
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A song on a "card" from an earlier write, and what a write of it needs.
    fn earlier(root: &Path) -> (PathBuf, Vec<Track>, Vec<PlanItem>, WriteProfile) {
        let card = root.join("card");
        fs::create_dir_all(card.join("A")).unwrap();
        fs::write(card.join("A/song.mp3"), [1u8; 9]).unwrap();
        fs::write(card.join("A/old.mp3"), [2u8; 7]).unwrap();
        // Its source has changed (another size) and, here, can't be read.
        let tracks = vec![Track { path: root.join("gone.mp3"), ext: "mp3".into(), size: 2000, ..Default::default() }];
        let plan = vec![PlanItem { track: 0, rel: PathBuf::from("A/song.mp3"), target: Target::Copy, skip: None }];
        let manifest = vec![Written { rel: PathBuf::from("A/song.mp3"), size: 9, source: None }, Written { rel: PathBuf::from("A/old.mp3"), size: 7, source: None }];
        (card, tracks, plan, WriteProfile { manifest, ..Default::default() })
    }

    #[test]
    fn a_write_gives_the_card_an_id_and_keeps_it() {
        let root = std::env::temp_dir().join(format!("pausetape-test-card-id-write-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let card = root.join("card");
        fs::create_dir_all(&card).unwrap();
        let w = WriteProfile::default();
        let write_once = || write(&w, &[], &[], &[], &root, &card, &AtomicBool::new(false), |_| {}).card_id;
        let first = write_once().unwrap();
        assert_eq!(first.len(), 16);
        assert_eq!(super::super::Label::read(&card).unwrap().card, first);
        assert_eq!(write_once().unwrap(), first, "the same card, the same id");
        fs::remove_dir_all(&card).unwrap();
        assert_eq!(write_once(), None, "no card, no id");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_card_that_is_gone_keeps_its_record_and_gets_no_folders() {
        let root = std::env::temp_dir().join(format!("pausetape-test-card-gone-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let (card, tracks, plan, w) = earlier(&root);
        fs::remove_dir_all(&card).unwrap();
        let out = write(&w, &tracks, &plan, &[], &root, &card, &AtomicBool::new(false), |_| {});
        assert!(out.card_gone && !out.stopped);
        assert_eq!(out.manifest, w.manifest, "nothing can be checked, so nothing is forgotten");
        assert!(!card.exists(), "no folder made where the card was");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_half_copy_left_by_a_pulled_card_goes_at_the_next_sync() {
        let root = std::env::temp_dir().join(format!("pausetape-test-half-copy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let card = root.join("card");
        // The song being copied when the card was pulled out, no longer chosen; and its album's
        // folder, which held nothing else.
        let half = super::super::copy::partial_path(&card.join("Band/Album/05 - Song.mp3"));
        fs::create_dir_all(half.parent().unwrap()).unwrap();
        fs::write(&half, [0u8; 64]).unwrap();
        let out = write(&WriteProfile::default(), &[], &[], &[], &root, &card, &AtomicBool::new(false), |_| {});
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert!(!half.exists() && !card.join("Band").exists(), "the half and its folders are gone");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(unix)]
    fn a_card_pulled_out_mid_copy_says_so_rather_than_the_systems_answer() {
        let root = std::env::temp_dir().join(format!("pausetape-test-pulled-mid-copy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let card = root.join("card");
        fs::create_dir_all(&card).unwrap();
        // A song that's a pipe: the copy waits on it, and the card goes while it waits.
        let song = root.join("song.mp3");
        assert!(std::process::Command::new("mkfifo").arg(&song).status().unwrap().success());
        let tracks = vec![Track { path: song.clone(), ext: "mp3".into(), size: 4, ..Default::default() }];
        let plan = vec![PlanItem { track: 0, rel: PathBuf::from("Band/Album/01 - Song.mp3"), target: Target::Copy, skip: None }];
        let pull = {
            let (song, card) = (song.clone(), card.clone());
            // Opening the pipe to write waits until the copy has opened it to read.
            std::thread::spawn(move || {
                let mut pipe = fs::OpenOptions::new().write(true).open(&song).unwrap();
                fs::remove_dir_all(&card).unwrap();
                std::io::Write::write_all(&mut pipe, b"song").unwrap();
            })
        };
        let out = write(&WriteProfile::default(), &tracks, &plan, &[], &root, &card, &AtomicBool::new(false), |_| {});
        pull.join().unwrap();
        assert!(out.card_gone && out.copied == 0);
        assert_eq!(out.errors, [format!("{}: the card went away while copying it", song.display())]);
        assert!(!card.exists(), "no folder made where the card was");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_song_whose_new_copy_fails_keeps_its_old_one() {
        let root = std::env::temp_dir().join(format!("pausetape-test-copy-fails-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let (card, tracks, plan, w) = earlier(&root);
        let out = write(&w, &tracks, &plan, &[], &root, &card, &AtomicBool::new(false), |_| {});
        assert!(!out.card_gone && !out.stopped && out.copied == 0 && !out.errors.is_empty());
        assert!(card.join("A/song.mp3").exists(), "still wanted: an old copy beats none");
        assert!(!card.join("A/old.mp3").exists(), "no longer wanted: removed as usual");
        assert_eq!(out.manifest.iter().map(|m| m.rel.clone()).collect::<Vec<_>>(), [PathBuf::from("A/song.mp3")], "the record says what's there");
        let _ = fs::remove_dir_all(&root);
    }


    /// `PAUSETAPE_FAT_CARD=<a FAT32 card or image, mounted> PAUSETAPE_FAT_SOURCE=<music> cargo test -- --ignored music_folder_on_fat --nocapture`
    /// Syncs everything to a real FAT32 card for a Walkman, then moves its folder around: MUSIC
    /// → Music (the same folder on FAT) → Audio/Music → the top. Each step must move, not copy,
    /// and every playlist line must reach a file. Writes to the card: use a spare or an image.
    #[test]
    #[ignore]
    fn music_folder_on_fat() {
        let (Some(card), Some(src)) = (std::env::var_os("PAUSETAPE_FAT_CARD").map(PathBuf::from), std::env::var_os("PAUSETAPE_FAT_SOURCE").map(PathBuf::from)) else { return };
        let (tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let playlists = vec![Playlist { name: "Mix: Best".into(), tracks: tracks.iter().step_by(7).map(Track::id).collect() }];
        let mut w = WriteProfile { name: "Walkman".into(), music_folder: "MUSIC".into(), options: Options { device: super::super::Device::Walkman, write_tags: false, ..Default::default() }, ..Default::default() };
        w.playlists.insert(playlists[0].name.clone());
        let songs = w.plan(&tracks, &index).iter().filter(|p| p.skip.is_none()).count();
        for (folder, moves) in [("MUSIC", false), ("Music", false), ("Audio/Music", true), ("", true)] {
            w.music_folder = folder.into();
            let started = std::time::Instant::now();
            let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
            println!("{folder:>12}: {} copied, {} moved, {} unchanged, {} removed, {} errors, {:.1?}", out.copied, out.renamed, out.unchanged, out.removed, out.errors.len(), started.elapsed());
            assert!(out.errors.is_empty(), "{:?}", out.errors);
            if w.manifest.is_empty() {
                assert_eq!(out.copied, songs, "the first Sync copies every song");
            } else {
                assert_eq!((out.copied, out.renamed), (0, if moves { songs } else { 0 }), "{folder}: moved, not copied");
            }
            w.manifest = out.manifest;
            let lists = w.playlists_dir();
            let m3u = fs::read_to_string(card.join(&lists).join("Mix- Best.m3u8")).or_else(|_| fs::read_to_string(card.join(&lists).join("Mix - Best.m3u8"))).unwrap();
            for l in m3u.lines().filter(|l| !l.starts_with('#')) {
                assert!(card.join(&lists).join(l).is_file(), "{folder}: {l}");
            }
            assert!(super::super::Label::read(&card).is_some(), "the label stays at the top");
            for m in &w.manifest {
                assert!(card.join(&m.rel).is_file(), "{folder}: {} recorded but not there", m.rel.display());
            }
        }
        let tops: Vec<String> = fs::read_dir(&card).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
        println!("top of the card at the end: {tops:?}");
        assert!(!tops.iter().any(|n| n == "MUSIC" || n == "Music" || n == "Audio"), "the folders emptied by the moves are gone");
    }

    /// `PAUSETAPE_CONVERT=<folder> [PAUSETAPE_CD=1] cargo test -- --ignored convert_write --nocapture`
    /// Writes a folder of songs RECHO can't play (ALAC, AAC, Dolby…) to a temporary "card" with
    /// conversion on, reports what each became, and checks that a second write converts nothing.
    #[test]
    #[ignore]
    fn convert_write() {
        use lofty::prelude::*;
        let Some(src) = std::env::var_os("PAUSETAPE_CONVERT").map(PathBuf::from) else { return };
        let card = std::env::temp_dir().join(format!("pausetape-test-convert-{}", std::process::id()));
        let _ = fs::remove_dir_all(&card);
        fs::create_dir_all(&card).unwrap();
        let (tracks, _) = crate::library::scan(&src, None, false, |_| {});
        let index = crate::library::grouping::build(&tracks, &Default::default());
        let mut w = WriteProfile::default();
        assert!(w.options.unplayable == super::super::Unplayable::Convert, "converting is the default");
        // PAUSETAPE_CD=1: hi-res at CD quality too.
        w.options.cd_quality = std::env::var_os("PAUSETAPE_CD").is_some();
        let plan = w.plan(&tracks, &index);
        for p in plan.iter().filter(|p| p.skip.is_some()) {
            println!("  left out: {} ({})", tracks[p.track].path.display(), p.skip.unwrap());
        }
        let t = std::time::Instant::now();
        let out = write(&w, &tracks, &plan, &[], &src, &card, &AtomicBool::new(false), |_| {});
        println!("first write: {} made in {:.1?}, errors {:?}", out.copied, t.elapsed(), out.errors);
        assert!(out.errors.is_empty());
        for (m, p) in out.manifest.iter().zip(plan.iter().filter(|p| p.skip.is_none())) {
            let f = lofty::read_from_path(card.join(&m.rel)).unwrap();
            let props = f.properties();
            let cover = f.tags().iter().any(|t| !t.pictures().is_empty());
            let title = f.primary_tag().and_then(|t| t.title().map(|s| s.into_owned())).unwrap_or_default();
            let src = &tracks[p.track];
            println!(
                "  {:<55} {:?} {} kbps {}/{} Hz · {:.1} s (source {:.1} s, {}) · title {title:?} · cover {cover}",
                m.rel.display().to_string(),
                p.target,
                props.audio_bitrate().unwrap_or(0),
                props.bit_depth().unwrap_or(0),
                props.sample_rate().unwrap_or(0),
                props.duration().as_secs_f32(),
                src.duration.as_secs_f32(),
                src.format_label()
            );
            assert!((props.duration().as_secs_f32() - src.duration.as_secs_f32()).abs() < 0.2, "same length as the source");
        }
        w.manifest = out.manifest;
        let again = write(&w, &tracks, &plan, &[], &src, &card, &AtomicBool::new(false), |_| {});
        println!("second write: {} made, {} unchanged", again.copied, again.unchanged);
        assert_eq!(again.copied, 0);
        let _ = fs::remove_dir_all(&card);
    }

    /// `PAUSETAPE_CUE_ALBUM=<folder with an album file + .cue> cargo test -- --ignored cue_album --nocapture`
    /// Writes the folder to a temporary "card": every song is cut into its own FLAC, and a second
    /// write cuts nothing.
    #[test]
    #[ignore]
    fn cue_album_write() {
        let Some(src) = std::env::var_os("PAUSETAPE_CUE_ALBUM").map(PathBuf::from) else { return };
        let card = std::env::temp_dir().join(format!("pausetape-test-cue-card-{}", std::process::id()));
        let _ = fs::remove_dir_all(&card);
        fs::create_dir_all(&card).unwrap();
        let (tracks, _) = crate::library::scan(&src, None, false, |_| {});
        assert!(tracks.iter().any(|t| t.cue.is_some()), "no album file with a CUE sheet in {}", src.display());
        let index = crate::library::grouping::build(&tracks, &Default::default());
        let mut w = WriteProfile::default();
        let plan = w.plan(&tracks, &index);
        let t = std::time::Instant::now();
        let out = write(&w, &tracks, &plan, &[], &src, &card, &AtomicBool::new(false), |_| {});
        println!("first write: {} cut/copied in {:.1?}, errors {:?}", out.copied, t.elapsed(), out.errors);
        assert!(out.errors.is_empty());
        for m in &out.manifest {
            let secs = lofty::read_from_path(card.join(&m.rel)).map(|f| lofty::prelude::AudioFile::properties(&f).duration().as_secs_f32()).unwrap_or(0.0);
            println!("  {}  {:.2} s", m.rel.display(), secs);
        }
        w.manifest = out.manifest;
        let p = preview(&w, &tracks, &index, &[], &card);
        let again = write(&w, &tracks, &plan, &[], &src, &card, &AtomicBool::new(false), |_| {});
        println!("preview: {} to copy, {} unchanged; second write: {} copied, {} unchanged", p.to_copy, p.unchanged, again.copied, again.unchanged);
        assert_eq!((p.to_copy, again.copied), (0, 0));
        let _ = fs::remove_dir_all(&card);
    }
    use crate::library::grouping::{ArtistPrefs, build};
    use crate::library::scan;

    #[test]
    fn stopped_write_records_what_is_on_the_card() {
        use std::sync::atomic::Ordering;
        let root = std::env::temp_dir().join(format!("pausetape-test-stop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
        fs::create_dir_all(&card).unwrap();
        for f in ["Muse/Absolution/01 - Intro.mp3", "Muse/Absolution/02 - Apocalypse Please.mp3", "Muse/Absolution/03 - Time Is Running Out.mp3"] {
            fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
            fs::write(src.join(f), f.as_bytes()).unwrap();
        }
        let (mut tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let mut w = WriteProfile { dest: Some(card.clone()), options: Options { write_tags: false, ..Default::default() }, ..Default::default() };
        let plan = w.plan(&tracks, &index);
        // Stop once the first song is on the card.
        fn stop_after_one(stop: &AtomicBool) -> impl FnMut(Progress) + '_ {
            move |p: Progress| {
                if let Progress::Step { done, .. } = p
                    && done >= 1
                {
                    stop.store(true, Ordering::Relaxed);
                }
            }
        }

        let stop = AtomicBool::new(false);
        let out = write(&w, &tracks, &plan, &[], &src, &card, &stop, stop_after_one(&stop));
        assert!(out.stopped);
        assert_eq!((out.copied, out.manifest.len()), (1, 1));
        assert!(card.join(&out.manifest[0].rel).exists());
        w.manifest = out.manifest;

        let out = write(&w, &tracks, &plan, &[], &src, &card, &AtomicBool::new(false), |_| {});
        assert!(!out.stopped);
        assert_eq!((out.copied, out.unchanged, out.manifest.len()), (2, 1, 3), "the next write carries on");
        w.manifest = out.manifest;

        // Everything to copy again, stopped after one: the songs it didn't reach are still the write's.
        for t in &mut tracks {
            t.dirty = true;
        }
        let stop = AtomicBool::new(false);
        let out = write(&w, &tracks, &plan, &[], &src, &card, &stop, stop_after_one(&stop));
        assert_eq!((out.copied, out.manifest.len()), (1, 3));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrected_songs_are_copied_once() {
        let root = std::env::temp_dir().join(format!("pausetape-test-corrected-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
        fs::create_dir_all(&card).unwrap();
        for f in ["Muse/Absolution/01 - Intro.mp3", "Muse/Absolution/02 - Apocalypse Please.mp3"] {
            fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
            fs::write(src.join(f), f.as_bytes()).unwrap();
        }
        let (mut tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let mut w = WriteProfile { options: Options { write_tags: false, ..Default::default() }, ..Default::default() };
        let sync = |w: &mut WriteProfile, tracks: &[Track]| {
            let out = write(w, tracks, &w.plan(tracks, &index), &[], &src, &card, &AtomicBool::new(false), |_| {});
            w.manifest = out.manifest;
            (out.copied, out.unchanged)
        };
        assert_eq!(sync(&mut w, &tracks), (2, 0));
        // A correction not yet in the music file: copied once, then left alone until it changes.
        tracks[0].year = Some(2003);
        tracks[0].dirty = true;
        assert_eq!(sync(&mut w, &tracks), (1, 1));
        assert_eq!(sync(&mut w, &tracks), (0, 2), "not copied again at every Sync");
        tracks[0].year = Some(2004);
        assert_eq!(sync(&mut w, &tracks), (1, 1));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn song_folders_keep_clear_of_the_apps_own() {
        let root = std::env::temp_dir().join(format!("pausetape-test-own-folders-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for f in ["_Playlists/Mix Tape/01 - One.mp3", "Pausetape format test/Demo/01 - Two.mp3", "Muse/Absolution/01 - Intro.mp3"] {
            fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
            fs::write(root.join(f), f.as_bytes()).unwrap();
        }
        let (tracks, _) = scan(&root, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        // A setup whose playlist copies go in "Playlists", the name the first artist takes.
        let w = WriteProfile { playlists_folder: "Playlists".into(), ..Default::default() };
        let mut firsts: Vec<String> = w.plan(&tracks, &index).iter().map(|p| p.rel.components().next().unwrap().as_os_str().to_string_lossy().into_owned()).collect();
        firsts.sort();
        assert_eq!(firsts, ["Muse", "Pausetape format test_", "Playlists_"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_pattern_renames_on_the_card() {
        let root = std::env::temp_dir().join(format!("pausetape-test-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
        fs::create_dir_all(&card).unwrap();
        for f in ["Muse/Absolution/01 - Intro.mp3", "Muse/Absolution/02 - Apocalypse Please.mp3", "Muse/Absolution/03 - Time Is Running Out.mp3"] {
            fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
            fs::write(src.join(f), f.as_bytes()).unwrap();
        }
        let (tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let playlists = vec![Playlist { name: "Mix".into(), tracks: vec![tracks[0].id()] }];
        let mut w = WriteProfile { dest: Some(card.clone()), options: Options { write_tags: false, ..Default::default() }, ..Default::default() };
        w.playlists.insert("Mix".into());
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert_eq!((out.copied, out.renamed), (4, 0));
        w.manifest = out.manifest;

        // A pattern of its own: the songs move on the card; the playlist's copy stays.
        w.options.pattern = Some("{albumartist} - {album}/{track}. {title}".into());
        let pv = preview(&w, &tracks, &index, &playlists, &card);
        assert_eq!((pv.renamed, pv.to_copy, pv.to_remove.len()), (3, 0, 0));
        // Renamed isn't removed: nothing to ask about. Choosing nothing would take all three off.
        assert_eq!(removing(&w, &tracks, &w.plan(&tracks, &index)), (0, 3));
        assert_eq!(removing(&w, &tracks, &[]), (3, 3));
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!((out.renamed, out.copied, out.unchanged, out.removed), (3, 0, 4, 0));
        assert!(card.join("Muse - Absolution/01. Intro.mp3").is_file() && card.join("_Playlists/Mix").is_dir());
        assert!(!card.join("Muse").exists(), "the old folders go once empty");
        w.manifest = out.manifest;

        // Back to the album order's names, with one card file changed behind the app's back
        // (same size): that one isn't trusted to be the song, so it's copied afresh.
        w.options.pattern = None;
        let changed = card.join("Muse - Absolution/02. Apocalypse Please.mp3");
        fs::write(&changed, vec![b'x'; fs::metadata(&changed).unwrap().len() as usize]).unwrap();
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert_eq!((out.renamed, out.copied), (2, 1));
        assert_eq!(fs::read(card.join("Muse/Absolution/02 - Apocalypse Please.mp3")).unwrap(), b"Muse/Absolution/02 - Apocalypse Please.mp3");
        assert!(!card.join("Muse - Absolution").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_music_folder_takes_the_songs_and_playlists_and_moves_what_is_there() {
        let root = std::env::temp_dir().join(format!("pausetape-test-music-folder-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
        fs::create_dir_all(&card).unwrap();
        for f in ["Muse/Absolution/01 - Intro.mp3", "Muse/Absolution/02 - Apocalypse Please.mp3"] {
            fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
            fs::write(src.join(f), f.as_bytes()).unwrap();
        }
        let (tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let playlists = vec![Playlist { name: "Mix".into(), tracks: vec![tracks[0].id()] }];
        let mut w = WriteProfile { dest: Some(card.clone()), options: Options { write_tags: false, ..Default::default() }, ..Default::default() };
        w.playlists.insert("Mix".into());
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert_eq!(out.copied, 3);
        assert!(card.join("Muse/Absolution/01 - Intro.mp3").is_file() && card.join("_Playlists/Mix.m3u8").is_file(), "no folder: the top, as before");
        w.manifest = out.manifest;

        // A Walkman's folder: the songs move into it, the playlist is made there afresh.
        w.music_folder = "MUSIC".into();
        assert_eq!(preview(&w, &tracks, &index, &playlists, &card).renamed, 2);
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!((out.renamed, out.copied), (2, 1));
        assert!(card.join("MUSIC/Muse/Absolution/02 - Apocalypse Please.mp3").is_file());
        assert!(!card.join("Muse").exists() && !card.join("_Playlists").exists(), "nothing left at the top");
        let m3u = fs::read_to_string(card.join("MUSIC/_Playlists/Mix.m3u8")).unwrap();
        assert!(m3u.contains("\n../../MUSIC/Muse/Absolution/0"), "the .m3u8 reaches the song from inside the folder: {m3u}");
        assert!(super::super::Label::read(&card).is_some(), "the label stays at the top");
        w.manifest = out.manifest;
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert_eq!((out.renamed, out.copied, out.removed), (0, 0, 0), "settled");
        w.manifest = out.manifest;

        // A player that reads playlists: the .m3u8 alone, the copy goes. A song of the playlist
        // not on the card otherwise is still copied beside it.
        w.options.device = super::super::Device::Walkman;
        let mut chosen = w.plan(&tracks, &index);
        chosen.retain(|p| p.track == 0);
        let playlists = vec![Playlist { name: "Mix".into(), tracks: vec![tracks[0].id(), tracks[1].id()] }];
        let pv = preview(&w, &tracks, &index, &playlists, &card);
        assert_eq!(pv.playlist_files, 0, "both songs are on the card in the setup's own plan");
        let out = write(&w, &tracks, &chosen, &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(out.copied, 1, "only the song the plan leaves out");
        let copies: Vec<_> = fs::read_dir(card.join("MUSIC/_Playlists/Mix")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(copies, [format!("02 - Muse - {}.mp3", tracks[1].display_title())]);
        let m3u = fs::read_to_string(card.join("MUSIC/_Playlists/Mix.m3u8")).unwrap();
        let lines: Vec<&str> = m3u.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("../../MUSIC/Muse/Absolution/") && lines[1].starts_with("Mix/02 - "), "{m3u}");
        for l in lines {
            assert!(card.join("MUSIC/_Playlists").join(l).is_file(), "{l} is there, as the player will look for it");
        }

        // What's typed is made safe for the card, a level at a time.
        let root_of = |f: &str| WriteProfile { music_folder: f.into(), ..Default::default() }.music_root();
        assert_eq!(root_of(" "), PathBuf::new());
        assert_eq!(root_of("/MUSIC/"), PathBuf::from("MUSIC"));
        assert_eq!(root_of("../Music"), PathBuf::from("Music"));
        assert_eq!(root_of("Audio\\Music: Mine"), PathBuf::from("Audio").join("Music - Mine"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rewrite_after_deselecting_artist_and_playlist_cleans_up() {
        let root = std::env::temp_dir().join(format!("pausetape-test-card-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
        fs::create_dir_all(&card).unwrap();
        for f in ["Muse/Absolution/01 - Intro.mp3", "Blur/Parklife/01 - Girls and Boys.mp3"] {
            fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
            fs::write(src.join(f), f.as_bytes()).unwrap();
        }
        fs::create_dir_all(&card).unwrap();
        fs::write(card.join("someone else's file.mp3"), b"keep me").unwrap();

        let (tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let playlists = vec![Playlist { name: "Mix".into(), tracks: vec![tracks[0].id()] }];
        let mut w = WriteProfile { dest: Some(card.clone()), options: Options { write_tags: false, ..Default::default() }, ..Default::default() };
        w.playlists.insert("Mix".into());

        let pv = preview(&w, &tracks, &index, &playlists, &card);
        assert_eq!((pv.to_copy, pv.unchanged, pv.playlist_files), (3, 0, 1));
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(out.manifest.len(), 4, "2 albums tracks + 1 playlist copy + 1 m3u8");
        w.manifest = out.manifest;

        let blur = index.artists.iter().find(|a| a.name == "Blur").unwrap();
        w.custom.excluded_artists.insert(index.clusters[blur.cluster].key.clone());
        w.playlists.clear();
        let pv = preview(&w, &tracks, &index, &playlists, &card);
        assert_eq!((pv.to_copy, pv.unchanged, pv.to_remove.len()), (0, 1, 3));
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, &AtomicBool::new(false), |_| {});
        assert!(!card.join("Blur").exists());
        assert!(!card.join("_Playlists/Mix.m3u8").exists());
        assert!(card.join("Muse/Absolution/01 - Intro.mp3").exists());
        assert!(card.join("someone else's file.mp3").exists());
        assert_eq!(out.manifest.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
