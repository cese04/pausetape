//! Application state and the glue between background work and the UI. Rendering lives in
//! `crate::ui`; this module owns the data, starts background work and folds its results back in.
//!
//! The `impl App` is split by topic: `music` (scanning, artists, MusicBrainz), `edit` (tags
//! edited by hand), `covers` (finding covers), `browse` (search and the album table), `add` (adding music from a folder), `cards` (noticing cards plugged in), `writes` (card writes, reorganising, playlists), `playback`, and `events`
//! (the per-frame message pump).

mod add;
mod browse;
mod cards;
mod covers;
mod edit;
mod events;
mod music;
mod playback;
mod testcard;
mod writes;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use serde::{Deserialize, Serialize};

pub use add::Adding;
pub use browse::{AlbumRow, SortBy};
pub use covers::FoundCover;
pub use edit::{Shared, TagEdit};

use crate::audio::{self, Audio};
use crate::card::playlist::{self, Playlist};
use crate::card::{self, Mode, Options, Outcome, PlanItem, Preview, Progress, Volume, WriteProfile};
use crate::library::db::Db;
use crate::library::grouping::{self, AlbumNode, ArtistPrefs, Index};
use crate::library::names::artist_key;
use crate::library::{self, ScanStats, Track};
use crate::musicbrainz::{self, ArtistHit, Field, FieldChange, Job, LocalTrack, Reply};

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub source: Option<PathBuf>,
    /// Saved card layouts; `active_write` indexes into it.
    pub writes: Vec<WriteProfile>,
    pub active_write: usize,
    /// Options for tidying the source folder in place (optional).
    pub tidy: Options,
    pub mb_contact: String,
    pub prefs: ArtistPrefs,
    pub playlists: Vec<Playlist>,
    pub volume: f32,
    pub theme: ThemePref,
    /// The library's format warning is collapsed to one line.
    pub format_note_collapsed: bool,
    /// The library shows a table of albums rather than the list by artist.
    pub album_table: bool,
    /// Searches kept for reuse ("quality:lossless jazz").
    pub searches: Vec<String>,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            source: None,
            writes: vec![WriteProfile::default()],
            active_write: 0,
            // Tidying keeps every format and leaves Finder files alone: it's the user's own disk.
            tidy: Options { unplayable: card::Unplayable::Copy, clean_mac_files: false, ..Options::default() },
            mb_contact: String::new(),
            prefs: ArtistPrefs::default(),
            playlists: Vec::new(),
            volume: 0.8,
            theme: ThemePref::System,
            format_note_collapsed: false,
            album_table: false,
            searches: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Folders,
    Library,
    Artists,
    Card,
    Playlists,
    Settings,
}

pub enum Lookup {
    Pending,
    Ready { label: String, changes: Vec<FieldChange> },
    Failed(String),
}

enum Msg {
    ScanProgress(usize),
    Scanned(PathBuf, Vec<Track>, ScanStats),
    Organize(Progress),
    WriteProgress(usize, usize),
    Written(String, Box<Outcome>),
    Previewed(String, Box<Preview>),
    /// What's on the card besides the write's own files, for taking it over.
    Surveyed(String, Box<card::adopt::Survey>),
    TookOver(String, Box<card::adopt::Outcome>),
    /// Music copied from the card into the music folder.
    ImportedFromCard(Box<card::adopt::Imported>),
    /// A folder of music to add, checked against the library.
    FolderSurveyed(PathBuf, Box<card::adopt::Survey>),
    /// Music copied from that folder into the music folder.
    Added(Box<card::adopt::Imported>),
    /// Card paths no longer the write's, files put back, errors.
    UndidTakeOver(String, Result<(Vec<PathBuf>, usize, Vec<String>), String>),
    /// Songs stored twice or album copies moved to the Trash: how many, of what, and what went wrong.
    Trashed(usize, &'static str, Vec<String>),
    /// (track index, new size, new mtime) for files whose tags were written.
    TagsSaved(Vec<(usize, u64, u64)>, Vec<String>),
    /// Loudness of a track, by track id.
    Envelope(PathBuf, Vec<f32>),
    /// Cover for a track path, and the image file it came from if it isn't embedded.
    Cover(PathBuf, Option<egui::ColorImage>, Option<PathBuf>),
    /// Covers found for albums without one, and whether the search was stopped.
    CoversFound(Vec<FoundCover>, bool),
    /// The test card's samples written to the card: what each became.
    TestCardWritten(Vec<(card::Sample, Result<(), String>)>),
    /// The test card's files taken off the card, or why not.
    TestCardRemoved(Result<(), String>),
}

pub struct Busy {
    pub label: String,
    pub done: usize,
    pub total: usize,
    /// When counting began and the count then: a write skips what's unchanged in one step, so
    /// the pace is measured from its first report, not from the start.
    since: Option<(Instant, usize)>,
    /// Set to ask the work to stop after the file it's on; `None` for work that can't stop halfway.
    pub stop: Option<Arc<AtomicBool>>,
}

impl Busy {
    pub fn new(label: impl Into<String>, total: usize) -> Busy {
        Busy { label: label.into(), done: 0, total, since: None, stop: None }
    }

    /// The same, with a flag the work checks between files.
    pub fn stoppable(label: impl Into<String>, total: usize) -> (Busy, Arc<AtomicBool>) {
        let stop = Arc::new(AtomicBool::new(false));
        (Busy { stop: Some(stop.clone()), ..Busy::new(label, total) }, stop)
    }

    pub fn progress(&mut self, done: usize, total: usize) {
        (self.done, self.total) = (done, total);
        self.since.get_or_insert((Instant::now(), done));
    }

    pub fn stopping(&self) -> bool {
        self.stop.as_ref().is_some_and(|s| s.load(Ordering::Relaxed))
    }

    /// For the screenshot demo: `done` of `total` after `secs` seconds.
    #[cfg(debug_assertions)]
    pub fn pretend(label: &str, done: usize, total: usize, secs: u64) -> Busy {
        let (mut b, _) = Busy::stoppable(label, total);
        b.since = Some((Instant::now() - Duration::from_secs(secs), 0));
        b.done = done;
        b
    }

    /// At the pace so far, once there's enough of it to go on (a few files and seconds).
    pub fn time_left(&self) -> Option<Duration> {
        let (at, from) = self.since?;
        let (done, secs) = (self.done.checked_sub(from)?, at.elapsed().as_secs_f64());
        if done < 3 || secs < 5.0 || self.total <= self.done {
            return None;
        }
        Some(Duration::from_secs_f64(secs / done as f64 * (self.total - self.done) as f64))
    }
}

/// The active write against its card, for the library's marks and the Sync strip. Works
/// without the card connected: "on the card" is what the write's manifest records.
#[derive(Default)]
pub struct CardState {
    /// Tracks on the card, as of the last write or take-over.
    pub on_card: HashSet<usize>,
    /// Tracks the write puts on the card as chosen now.
    pub planned: HashSet<usize>,
    /// Albums Sync adds (or completes), swaps for another copy, and takes off.
    pub added: usize,
    pub replaced: usize,
    pub removed: usize,
    /// Songs on the card twice (see `AlbumNode::repeats`): Sync takes the extra copies off.
    pub repeats: usize,
    /// Songs on the card under an old name (another pattern, a renamed album): Sync moves them.
    pub renamed: usize,
    /// Bytes Sync copies, and frees.
    pub add_bytes: u64,
    pub remove_bytes: u64,
    /// What the chosen music takes on the card, playlist copies included.
    pub planned_bytes: u64,
}

/// Files a card operation couldn't handle, and where the full list was saved.
pub struct Problems {
    pub what: String,
    pub list: Vec<String>,
    pub log: Option<PathBuf>,
}

pub struct App {
    pub saved: Saved,
    pub tracks: Vec<Track>,
    pub index: Index,
    pub tab: Tab,
    pub filter: String,
    /// The library shows only albums with a copy that another copy covers.
    pub duplicates_only: bool,
    /// Expanded artists in the library, by artist key.
    pub expanded: HashSet<String>,
    /// Songs chosen in the library (⌘-click), for editing their tags together.
    pub selected: BTreeSet<usize>,
    /// The song last clicked into the selection, where a shift-click run starts.
    pub select_anchor: Option<usize>,
    /// The open tag editor.
    pub tag_edit: Option<TagEdit>,
    /// Albums the last cover search found nothing for, and why.
    pub cover_misses: Vec<(String, String)>,
    /// The album table's order: column, and largest/last first.
    pub album_sort: (SortBy, bool),
    /// Albums chosen in the table (by `AlbumNode::id`), and the last one clicked.
    pub selected_albums: BTreeSet<String>,
    pub album_anchor: Option<String>,
    /// The dialog for taking over music already on the card is open.
    pub take_over_open: bool,
    /// A folder of new music checked against the library, waiting to be added.
    pub adding: Option<Adding>,
    /// The naming pattern being typed in the card tab, while it isn't valid yet.
    pub pattern_draft: Option<String>,
    /// The dialog for testing the card's player.
    pub player_test_open: bool,
    /// What the test card's samples became the last time it was written.
    pub test_card: Option<Vec<(card::Sample, Result<(), String>)>>,
    pub busy: Option<Busy>,
    pub notices: Vec<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    mb: Option<(String, musicbrainz::Client)>,
    mb_tx: Sender<Reply>,
    mb_rx: Receiver<Reply>,
    /// MusicBrainz artist candidates per cluster key; `None` while pending.
    pub artist_hits: HashMap<String, Option<Result<Vec<ArtistHit>, String>>>,
    pub lookups: HashMap<String, Lookup>,
    /// Plan for tidying the source in place, once previewed.
    pub tidy_plan: Option<Vec<PlanItem>>,
    /// How many tracks already sit where tidying would put them.
    pub in_place: usize,
    /// Preview of the active write, tagged with the write's name.
    pub write_preview: Option<(String, Preview)>,
    /// The active write against its card: what's there, what's chosen, what Sync changes.
    pub card_state: CardState,
    /// The card surveyed for taking over, by write name.
    pub card_survey: Option<(String, card::adopt::Survey)>,
    /// The room a running write started with: its new files fill the card before the manifest
    /// counts them, so the live figure would shrink as it goes.
    pub room_while_writing: Option<u64>,
    /// Gather what a take-over doesn't use into one folder on the card.
    pub gather_on_take_over: bool,
    /// Delete clutter (iTunes data, system files, rip logs) when taking over a card.
    pub delete_clutter_on_take_over: bool,
    /// What went wrong in the last card operation, listed in the card tab.
    pub problems: Option<Problems>,
    /// Open folder/card dialog.
    pub picker: Option<crate::ui::folders::Picker>,
    /// Library A–Z rail: jump to the first artist under this letter on the next frame.
    pub jump_to: Option<char>,
    pub volumes: Vec<Volume>,
    /// Drives mounted at the last look, and when that was (`cards`).
    mounted: HashSet<PathBuf>,
    last_watch: Option<Instant>,
    /// The label on the active setup's card, as of the last look.
    pub dest_label: Option<card::Label>,
    pub card_filter: String,
    pub card_expanded: HashSet<String>,
    /// Inline rename on the card tab: ("artist"/"album", key, text).
    pub card_renaming: Option<(bool, String, String)>,
    pub audio: Audio,
    pub queue: Vec<usize>,
    pub qpos: usize,
    /// Waveform of the playing track, by track id.
    pub envelope: Option<(PathBuf, Vec<f32>)>,
    /// Cover of the playing track; `None` inside once looked up and none was found.
    pub cover: Option<(PathBuf, Option<egui::TextureHandle>, Option<PathBuf>)>,
    pub active_playlist: Option<usize>,
    pub renaming: Option<(usize, String)>,
    /// UI-thread connection to the library database (scans open their own).
    pub db: Option<Db>,
    ctx: egui::Context,
    #[cfg(debug_assertions)]
    shooter: Option<crate::ui::screenshots::Shooter>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::ui::theme::install(&cc.egui_ctx);
        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        let ctx = cc.egui_ctx.clone();
        let wake = ctx.clone();
        let audio = Audio::spawn(move || wake.request_repaint());
        audio.volume(saved.volume);
        let (tx, rx) = unbounded();
        let (mb_tx, mb_rx) = unbounded();
        let mut saved = saved;
        if saved.writes.is_empty() {
            saved.writes.push(WriteProfile::default());
        }
        saved.active_write = saved.active_write.min(saved.writes.len() - 1);
        let first_run = saved.source.is_none();
        let mut app = App {
            active_playlist: (!saved.playlists.is_empty()).then_some(0),
            saved,
            tracks: Vec::new(),
            index: Index::default(),
            tab: if first_run { Tab::Folders } else { Tab::Library },
            filter: String::new(),
            duplicates_only: false,
            expanded: HashSet::new(),
            selected: BTreeSet::new(),
            select_anchor: None,
            tag_edit: None,
            cover_misses: Vec::new(),
            album_sort: (SortBy::Artist, false),
            selected_albums: BTreeSet::new(),
            album_anchor: None,
            take_over_open: false,
            adding: None,
            pattern_draft: None,
            player_test_open: false,
            test_card: None,
            busy: None,
            notices: Vec::new(),
            tx,
            rx,
            mb: None,
            mb_tx,
            mb_rx,
            artist_hits: HashMap::new(),
            lookups: HashMap::new(),
            tidy_plan: None,
            in_place: 0,
            write_preview: None,
            card_survey: None,
            room_while_writing: None,
            card_state: CardState::default(),
            gather_on_take_over: true,
            delete_clutter_on_take_over: false,
            problems: None,
            picker: None,
            jump_to: None,
            volumes: card::volumes(),
            mounted: HashSet::new(),
            last_watch: None,
            dest_label: None,
            card_filter: String::new(),
            card_expanded: HashSet::new(),
            card_renaming: None,
            audio,
            queue: Vec::new(),
            qpos: 0,
            envelope: None,
            cover: None,
            renaming: None,
            db: Db::open_default().ok(),
            ctx,
            #[cfg(debug_assertions)]
            shooter: crate::ui::screenshots::Shooter::from_env(),
        };
        app.apply_theme();
        app.scan();
        app
    }

    pub fn apply_theme(&self) {
        self.ctx.set_theme(match self.saved.theme {
            ThemePref::System => egui::ThemePreference::System,
            ThemePref::Light => egui::ThemePreference::Light,
            ThemePref::Dark => egui::ThemePreference::Dark,
        });
    }

    /// Keep the errors of a card operation for the card tab, saved to a file too, and return the
    /// note for its notice ("" if there were none).
    pub fn report_problems(&mut self, what: &str, errors: Vec<String>) -> String {
        if errors.is_empty() {
            return String::new();
        }
        let note = format!(" · {} {}, listed in the card tab", errors.len(), if errors.len() == 1 { "problem" } else { "problems" });
        let log = card::save_problems(what, &errors);
        self.problems = Some(Problems { what: what.into(), list: errors, log });
        note
    }

    pub fn notice(&mut self, s: impl Into<String>) {
        self.notices.push(s.into());
        if self.notices.len() > 4 {
            self.notices.remove(0);
        }
    }

    fn spawn(&self, work: impl FnOnce(Sender<Msg>) + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            work(tx);
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_left_waits_for_a_pace_then_extrapolates() {
        let mut b = Busy::new("writing", 1000);
        b.progress(400, 1000);
        assert_eq!(b.time_left(), None, "the first report (what's unchanged) sets where counting starts");
        b.since = Some((Instant::now() - Duration::from_secs(60), 400));
        b.progress(500, 1000);
        let left = b.time_left().unwrap().as_secs();
        assert!((299..=301).contains(&left), "100 songs a minute, 500 to go: {left} s");
        b.progress(1000, 1000);
        assert_eq!(b.time_left(), None);
    }
}

fn plural_files(n: usize) -> String {
    if n == 1 { "1 file".into() } else { format!("{n} files") }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        self.watch_cards();
        #[cfg(debug_assertions)]
        if let Some(mut s) = self.shooter.take() {
            s.tick(self, ui.ctx());
            self.shooter = Some(s);
        }
        crate::ui::show(self, ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // The screenshot demo rewrites playlists and the active write; never persist that.
        #[cfg(debug_assertions)]
        if self.shooter.is_some() {
            return;
        }
        eframe::set_value(storage, eframe::APP_KEY, &self.saved);
    }
}
