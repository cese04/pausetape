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
}

impl Default for WriteProfile {
    fn default() -> Self {
        WriteProfile {
            id: super::label::new_id(),
            name: "Echo Mini".into(),
            dest: None,
            options: Options::default(),
            custom: Custom::default(),
            playlists: BTreeSet::new(),
            playlists_folder: "_Playlists".into(),
            prune: true,
            manifest: Vec::new(),
            last_written: None,
            take_over_offered: false,
        }
    }
}

impl WriteProfile {
    pub fn plan(&self, tracks: &[Track], index: &Index) -> Vec<PlanItem> {
        let mut plan = super::build_plan(tracks, index, &self.options, &self.custom);
        // Song folders keep clear of the folders the app keeps for itself on the card (an
        // artist called "_Playlists"): they'd be taken for its playlist copies or test samples.
        let own = [sanitize_component(&self.playlists_folder, 80).to_lowercase(), super::testcard::TEST_FOLDER.to_lowercase()];
        for p in &mut plan {
            let mut parts = p.rel.components();
            let Some(first) = parts.next().map(|c| c.as_os_str().to_string_lossy().into_owned()) else { continue };
            if parts.clone().next().is_some() && own.contains(&first.to_lowercase()) {
                p.rel = PathBuf::from(format!("{first}_")).join(parts.as_path());
            }
        }
        plan
    }

    pub fn chosen_playlists(&self, all: &[Playlist]) -> Vec<Playlist> {
        all.iter().filter(|p| self.playlists.contains(&p.name)).cloned().collect()
    }

    pub fn manifest_bytes(&self) -> u64 {
        self.manifest.iter().map(|w| w.size).sum()
    }

    /// A card file of this write that's one of its playlist copies.
    fn in_playlists(&self, rel: &Path) -> bool {
        let dir = sanitize_component(&self.playlists_folder, 80).to_lowercase();
        Path::new(&rel.to_string_lossy().to_lowercase()).starts_with(&dir)
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
    for (dir, copies, _) in playlist::planned(&w.chosen_playlists(playlists), &by_id, &w.playlists_folder, w.options.rules()) {
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
}

/// Remove what the previous write left that the plan no longer wants, copy the plan and
/// playlists to the card, then remove what's left over (playlist files, songs that failed).
///
/// Once `stop` is set, the song being copied is finished and the write ends there. Its
/// manifest then holds what it copied plus the earlier files still on the card, so the next
/// write carries on from there and can still remove anything of its own.
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
    // Labelled first, so a write stopped halfway still says whose card it is.
    let labelled = super::Label::of(w).save(dest).map_err(|e| format!("{e:#}"));
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
    // Clean the macOS files once at the end, after playlists too.
    let opts = Options { clean_mac_files: false, ..w.options.clone() };
    let mut r = super::execute(tracks, plan, source, dest, Mode::CopyTo, &opts, &w.manifest, stop, &mut progress);
    r.errors.extend(labelled.err());
    r.errors.extend(rename_errors);
    if r.stopped {
        let mut manifest = r.written;
        let mut have: HashSet<String> = manifest.iter().map(|m| m.rel.to_string_lossy().to_lowercase()).collect();
        manifest.extend(w.manifest.iter().filter(|m| have.insert(m.rel.to_string_lossy().to_lowercase()) && dest.join(&m.rel).exists()).cloned());
        let cleaned = if w.options.clean_mac_files { super::clean_mac_files(dest) } else { 0 };
        return Outcome { manifest, copied: r.copied, unchanged: r.unchanged, skipped: r.skipped, tagged: r.tagged, removed: early, renamed, cleaned, errors: r.errors, stopped: true };
    }

    let organised: HashMap<PathBuf, PathBuf> = plan.iter().filter(|p| p.skip.is_none()).map(|p| (tracks[p.track].id(), p.rel.clone())).collect();
    let pl = playlist::export(&w.chosen_playlists(playlists), tracks, &organised, dest, &w.playlists_folder, w.options.rules(), &w.manifest, |_, _| {});

    let mut manifest = r.written;
    manifest.extend(pl.written);
    let keep: HashSet<String> = manifest.iter().map(|m| m.rel.to_string_lossy().to_lowercase()).collect();
    let removed = if w.prune { super::prune(dest, &w.manifest, &keep) } else { 0 };
    let cleaned = if w.options.clean_mac_files { super::clean_mac_files(dest) } else { 0 };

    let mut errors = r.errors;
    errors.extend(pl.errors);
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
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// `NEATLIBRARY_CONVERT=<folder> [NEATLIBRARY_CD=1] cargo test -- --ignored convert_write --nocapture`
    /// Writes a folder of songs RECHO can't play (ALAC, AAC, Dolby…) to a temporary "card" with
    /// conversion on, reports what each became, and checks that a second write converts nothing.
    #[test]
    #[ignore]
    fn convert_write() {
        use lofty::prelude::*;
        let Some(src) = std::env::var_os("NEATLIBRARY_CONVERT").map(PathBuf::from) else { return };
        let card = std::env::temp_dir().join(format!("neatlibrary-test-convert-{}", std::process::id()));
        let _ = fs::remove_dir_all(&card);
        fs::create_dir_all(&card).unwrap();
        let (tracks, _) = crate::library::scan(&src, None, false, |_| {});
        let index = crate::library::grouping::build(&tracks, &Default::default());
        let mut w = WriteProfile::default();
        assert!(w.options.unplayable == super::super::Unplayable::Convert, "converting is the default");
        // NEATLIBRARY_CD=1: hi-res at CD quality too.
        w.options.cd_quality = std::env::var_os("NEATLIBRARY_CD").is_some();
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

    /// `NEATLIBRARY_CUE_ALBUM=<folder with an album file + .cue> cargo test -- --ignored cue_album --nocapture`
    /// Writes the folder to a temporary "card": every song is cut into its own FLAC, and a second
    /// write cuts nothing.
    #[test]
    #[ignore]
    fn cue_album_write() {
        let Some(src) = std::env::var_os("NEATLIBRARY_CUE_ALBUM").map(PathBuf::from) else { return };
        let card = std::env::temp_dir().join(format!("neatlibrary-test-cue-card-{}", std::process::id()));
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
        let root = std::env::temp_dir().join(format!("neatlibrary-test-stop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
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
        let root = std::env::temp_dir().join(format!("neatlibrary-test-corrected-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
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
        let root = std::env::temp_dir().join(format!("neatlibrary-test-own-folders-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for f in ["_Playlists/Mix Tape/01 - One.mp3", "NeatLibrary format test/Demo/01 - Two.mp3", "Muse/Absolution/01 - Intro.mp3"] {
            fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
            fs::write(root.join(f), f.as_bytes()).unwrap();
        }
        let (tracks, _) = scan(&root, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        // A setup whose playlist copies go in "Playlists", the name the first artist takes.
        let w = WriteProfile { playlists_folder: "Playlists".into(), ..Default::default() };
        let mut firsts: Vec<String> = w.plan(&tracks, &index).iter().map(|p| p.rel.components().next().unwrap().as_os_str().to_string_lossy().into_owned()).collect();
        firsts.sort();
        assert_eq!(firsts, ["Muse", "NeatLibrary format test_", "Playlists_"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_pattern_renames_on_the_card() {
        let root = std::env::temp_dir().join(format!("neatlibrary-test-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
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
    fn rewrite_after_deselecting_artist_and_playlist_cleans_up() {
        let root = std::env::temp_dir().join(format!("neatlibrary-test-card-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, card) = (root.join("src"), root.join("card"));
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
