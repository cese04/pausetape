//! Saved "writes": named layouts for an SD card, and the sync that applies one.
//!
//! A write remembers what goes on the card (artists, albums, playlists), what to call it, in what
//! order, and a manifest of the files it put there last time. The manifest is what makes it safe
//! to remove deselected music: only files this write created (and that still have the size it
//! recorded) are ever deleted.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::playlist::{self, Playlist};
use super::plan::{Target, card_size};
use super::{Custom, Mode, Options, PlanItem, Progress, Written};
use crate::library::Track;
use crate::library::grouping::Index;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WriteProfile {
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
}

impl Default for WriteProfile {
    fn default() -> Self {
        WriteProfile {
            name: "Echo Mini".into(),
            dest: None,
            options: Options::default(),
            custom: Custom::default(),
            playlists: BTreeSet::new(),
            playlists_folder: "_Playlists".into(),
            prune: true,
            manifest: Vec::new(),
            last_written: None,
        }
    }
}

impl WriteProfile {
    pub fn plan(&self, tracks: &[Track], index: &Index) -> Vec<PlanItem> {
        super::build_plan(tracks, index, &self.options, &self.custom)
    }

    pub fn chosen_playlists(&self, all: &[Playlist]) -> Vec<Playlist> {
        all.iter().filter(|p| self.playlists.contains(&p.name)).cloned().collect()
    }

    pub fn manifest_bytes(&self) -> u64 {
        self.manifest.iter().map(|w| w.size).sum()
    }
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
    for item in w.plan(tracks, index) {
        if item.skip.is_some() {
            p.skipped += 1;
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
        p.to_remove = w.manifest.iter().filter(|m| !keep.contains(&m.rel.to_string_lossy().to_lowercase())).map(|m| m.rel.clone()).collect();
    }
    let foreign = super::adopt::foreign_files(dest, &w.manifest);
    (p.foreign, p.foreign_bytes) = (foreign.len(), foreign.iter().map(|(_, n)| n).sum());
    p.can_undo_take_over = super::adopt::latest(&w.name).is_some();
    p
}

pub struct Outcome {
    pub manifest: Vec<Written>,
    pub copied: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub tagged: usize,
    pub removed: usize,
    pub cleaned: usize,
    pub errors: Vec<String>,
}

/// Copy the plan and playlists to the card, then remove what the previous write left behind.
pub fn write(
    w: &WriteProfile,
    tracks: &[Track],
    plan: &[PlanItem],
    playlists: &[Playlist],
    source: &Path,
    dest: &Path,
    mut progress: impl FnMut(Progress),
) -> Outcome {
    // Clean the macOS files once at the end, after playlists too.
    let opts = Options { clean_mac_files: false, ..w.options.clone() };
    let r = super::execute(tracks, plan, source, dest, Mode::CopyTo, &opts, &w.manifest, &mut progress);

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
        removed: removed + pl.removed,
        cleaned,
        errors,
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
        let out = write(&w, &tracks, &plan, &[], &src, &card, |_| {});
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
        let again = write(&w, &tracks, &plan, &[], &src, &card, |_| {});
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
        let (tracks, _) = crate::library::scan(&src, None, false, |_| {});
        assert!(tracks.iter().any(|t| t.cue.is_some()), "no album file with a CUE sheet in {}", src.display());
        let index = crate::library::grouping::build(&tracks, &Default::default());
        let mut w = WriteProfile::default();
        let plan = w.plan(&tracks, &index);
        let t = std::time::Instant::now();
        let out = write(&w, &tracks, &plan, &[], &src, &card, |_| {});
        println!("first write: {} cut/copied in {:.1?}, errors {:?}", out.copied, t.elapsed(), out.errors);
        assert!(out.errors.is_empty());
        for m in &out.manifest {
            let secs = lofty::read_from_path(card.join(&m.rel)).map(|f| lofty::prelude::AudioFile::properties(&f).duration().as_secs_f32()).unwrap_or(0.0);
            println!("  {}  {:.2} s", m.rel.display(), secs);
        }
        w.manifest = out.manifest;
        let p = preview(&w, &tracks, &index, &[], &card);
        let again = write(&w, &tracks, &plan, &[], &src, &card, |_| {});
        println!("preview: {} to copy, {} unchanged; second write: {} copied, {} unchanged", p.to_copy, p.unchanged, again.copied, again.unchanged);
        assert_eq!((p.to_copy, again.copied), (0, 0));
        let _ = fs::remove_dir_all(&card);
    }
    use crate::library::grouping::{ArtistPrefs, build};
    use crate::library::scan;

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
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, |_| {});
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(out.manifest.len(), 4, "2 albums tracks + 1 playlist copy + 1 m3u8");
        w.manifest = out.manifest;

        let blur = index.artists.iter().find(|a| a.name == "Blur").unwrap();
        w.custom.excluded_artists.insert(index.clusters[blur.cluster].key.clone());
        w.playlists.clear();
        let pv = preview(&w, &tracks, &index, &playlists, &card);
        assert_eq!((pv.to_copy, pv.unchanged, pv.to_remove.len()), (0, 1, 3));
        let out = write(&w, &tracks, &w.plan(&tracks, &index), &playlists, &src, &card, |_| {});
        assert!(!card.join("Blur").exists());
        assert!(!card.join("_Playlists/Mix.m3u8").exists());
        assert!(card.join("Muse/Absolution/01 - Intro.mp3").exists());
        assert!(card.join("someone else's file.mp3").exists());
        assert_eq!(out.manifest.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
