//! Application state and the glue between background work and the UI. Rendering lives in
//! `crate::ui`; this module owns the data, starts background work and folds its results back in.
//!
//! The `impl App` is split by topic: `music` (scanning, artists, MusicBrainz), `edit` (tags
//! edited by hand), `covers` (finding covers), `browse` (search and the album table), `add` (adding music from a folder), `cards` (noticing cards plugged in), `writes` (card writes, reorganising, playlists), `undo` (taking back changes to playlists and to what goes on the card), `playback`, and `events`
//! (the per-frame message pump).

mod add;
mod browse;
mod cards;
mod covers;
mod edit;
mod events;
mod genres;
mod lyrics;
mod music;
mod playback;
mod testcard;
mod undo;
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
pub use genres::GenreIndex;
pub use lyrics::LyricsState;
pub use undo::Undo;

use crate::audio::{self, Audio};
use crate::card::playlist::{self, Playlist};
use crate::card::{self, Mode, Options, Outcome, PlanItem, Preview, Progress, Volume, WriteProfile};
use crate::library::db::Db;
use crate::library::grouping::{self, AlbumNode, ArtistPrefs, Index, UNKNOWN, VARIOUS};
use crate::library::names::{artist_key, primary_artist};
use crate::library::{self, ScanStats, Track};
use crate::musicbrainz::{self, ArtistFound, ArtistHit, ArtistInfo, Field, FieldChange, Job, LocalTrack, Reply};

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

/// The text sizes offered, smallest and largest (egui's zoom; Settings steps through them by tenths).
pub const TEXT_SIZES: (f32, f32) = (0.8, 2.0);

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
    pub accent: crate::ui::theme::Accent,
    /// The library shows a table of albums rather than the list by artist.
    pub album_table: bool,
    /// Searches kept for reuse ("quality:lossless jazz").
    pub searches: Vec<String>,
    /// The player folded to one line (a click on its cover).
    pub compact_player: bool,
    /// The size of everything, 1 being as designed (egui's zoom: ⌘+, ⌘−, ⌘0, or Settings).
    pub text_size: f32,
    /// Lists packed tight, their rows 24 apart rather than 32.
    pub compact: bool,
    /// No movement: changes at once, the reels still (the system's wish is followed too).
    pub reduce_motion: bool,
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
            accent: Default::default(),
            album_table: true,
            searches: Vec::new(),
            compact_player: false,
            text_size: 1.0,
            compact: false,
            reduce_motion: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Library,
    /// MusicBrainz suggestions for every album looked up, to go through.
    Suggestions,
    /// Every genre, to open one's artists and albums.
    Genres,
    Card,
    Playlists,
    Settings,
}

pub enum ArtistState {
    /// Not in the library database: asked when the artist is opened, or with "look up".
    Unasked,
    Asking,
    Found { info: ArtistInfo, fetched: u64 },
    /// Several artists by that name, or none close: which one is it?
    Choose(Vec<ArtistHit>),
    Failed(String),
}

pub enum Lookup {
    /// Asked; the ids of the songs sent, in the order the reply refers to them by.
    Pending(Vec<PathBuf>),
    Ready { label: String, changes: Vec<FieldChange> },
    Failed(String),
}

enum Msg {
    ScanProgress(usize),
    Scanned(PathBuf, Vec<Track>, ScanStats),
    Organize(Progress),
    WriteProgress(usize, usize),
    /// A finished write, and the others below, by setup id (`WriteProfile::id`): names change.
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
    /// Cover of the album the details panel shows, by its first song's path.
    DetailCover(PathBuf, Option<egui::ColorImage>),
    /// A song's lyrics (by track id), none found, or why they couldn't be looked for.
    Lyrics(PathBuf, Result<Option<crate::lyrics::Lyrics>, String>),
    /// An album's or selection's lyrics looked for: found, none, failed.
    LyricsDone(usize, usize, usize),
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
    /// Tracks copied onto the card by hand, as the last check of the card found them; until
    /// the card is taken over, Sync doesn't count them as there.
    pub by_hand: HashSet<usize>,
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

/// A change that can't be undone, waiting for a yes.
#[derive(Clone)]
pub enum Confirm {
    /// Deleting a card setup, by id: its record of the files it put on the card goes with it.
    DeleteWrite(String),
    /// Forgetting the tags the library database keeps, so every file is read again.
    ForgetTags,
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
    /// The library shows only artists whose names look like another's, to decide on.
    pub look_alikes_only: bool,
    /// Expanded artists in the library, by artist key.
    pub expanded: HashSet<String>,
    /// Artists whose links to stores and sites are shown, by cluster key (folded otherwise).
    pub artist_links_open: HashSet<String>,
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
    /// The music folder couldn't be read at the last scan (its drive not connected?). The library
    /// keeps what it had, and work that relies on it waits (`library_unavailable`).
    pub source_missing: bool,
    /// A Sync that would take most of the setup's songs off the card, waiting for a yes:
    /// (setup id, songs it removes, songs the setup has there).
    pub sync_confirm: Option<(String, usize, usize)>,
    /// The naming pattern being typed in the card tab, while it isn't valid yet.
    pub pattern_draft: Option<String>,
    /// The dialog for testing the card's player.
    pub player_test_open: bool,
    /// What the test card's samples became the last time it was written.
    pub test_card: Option<Vec<(card::Sample, Result<(), String>)>>,
    pub busy: Option<Busy>,
    pub notices: Vec<String>,
    /// Changes to playlists and to what goes on the card, latest last, for ⌘Z.
    pub undo_stack: Vec<Undo>,
    /// The notice about the latest of them, which offers "undo" while it's showing.
    pub undo_notice: Option<String>,
    /// A dialog asking before a change that can't be undone.
    pub confirm: Option<Confirm>,
    /// A folder dropped on the window while other work ran: checked once it's done.
    pub pending_add: Option<PathBuf>,
    /// ⌘F was pressed: the open tab's search box takes the keyboard on its next frame.
    pub focus_search: bool,
    /// A song or album row (album table position) chosen from the keyboard, to scroll into view.
    pub scroll_to_song: Option<usize>,
    pub album_scroll_to: Option<usize>,
    /// The album table rows drawn last frame, to tell whether one is in view.
    pub album_rows_shown: std::ops::Range<usize>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    mb: Option<(String, musicbrainz::Client)>,
    mb_tx: Sender<Reply>,
    mb_rx: Receiver<Reply>,
    /// The artist panel's MusicBrainz entry, by cluster key.
    pub artist_info: HashMap<String, ArtistState>,
    /// Every stored entry has been read from the library database (for the genres view).
    artists_loaded: bool,
    /// Every genre with its albums, made again when the library changes (`genre_index`).
    genre_index: GenreIndex,
    /// The genres view lists the genres with most albums first, rather than by name.
    pub genres_by_size: bool,
    pub lookups: HashMap<String, Lookup>,
    /// Plan for tidying the source in place, once previewed.
    pub tidy_plan: Option<Vec<PlanItem>>,
    /// How many tracks already sit where tidying would put them.
    pub in_place: usize,
    /// Preview of the active write, tagged with the setup's id.
    pub write_preview: Option<(String, Preview)>,
    /// The active write against its card: what's there, what's chosen, what Sync changes.
    pub card_state: CardState,
    /// The card surveyed for taking over, by setup id.
    pub card_survey: Option<(String, card::adopt::Survey)>,
    /// The library songs that check found copied onto the card by hand, by setup id and card
    /// folder. Kept while the selection changes (the survey isn't), for the card tab's marks.
    pub card_found: Option<(String, PathBuf, HashSet<usize>)>,
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
    /// The card tab lists only the albums the next Sync adds or takes off.
    pub card_changes_only: bool,
    /// The card view shows how the card is set up, rather than what's on it.
    pub card_setup_open: bool,
    pub card_expanded: HashSet<String>,
    /// Inline rename on the card tab: ("artist"/"album", key, text).
    pub card_renaming: Option<(bool, String, String)>,
    pub audio: Audio,
    /// Counts library rebuilds, so work derived from the library knows when to redo itself.
    pub library_generation: u64,
    /// When the music folder was last read through (seconds since 1970).
    pub library_read: Option<u64>,
    pub queue: Vec<usize>,
    pub qpos: usize,
    /// Waveform of the playing track, by track id.
    pub envelope: Option<(PathBuf, Vec<f32>)>,
    /// Cover of the playing track; `None` inside once looked up and none was found.
    pub cover: Option<(PathBuf, Option<egui::TextureHandle>, Option<PathBuf>)>,
    pub active_playlist: Option<usize>,
    /// The cover the details panel shows, by its album's first song (`None` inside while it
    /// loads, or when there's none).
    pub detail_cover: Option<(PathBuf, Option<egui::TextureHandle>)>,
    /// The details of what's selected, as a sheet over the page (when it has no room beside).
    pub details_sheet: bool,
    /// The details panel had room beside the library on the last frame.
    pub details_beside: bool,
    /// The system asks apps for less motion (read at start).
    pub system_reduce_motion: bool,
    /// The first-run welcome (music folder, card) is open.
    pub welcome_open: bool,
    /// Songs' lyrics, by track id, as far as they're known (`lyrics_of`).
    pub lyrics: HashMap<PathBuf, LyricsState>,
    lyrics_tx: Option<Sender<lyrics::LyricsJob>>,
    /// The details panel shows lyrics rather than the songs, and of which song.
    pub details_lyrics: bool,
    pub lyrics_song: Option<usize>,
    /// The panel keeps the line being sung in view: until the panel is scrolled by hand, and
    /// again on *Follow the song*, a click on a line, or the next song.
    pub lyrics_follow: bool,
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
        // Screenshots of a music folder that can't be reached (never saved: see `save`).
        #[cfg(debug_assertions)]
        if let Some(dir) = std::env::var_os("PAUSETAPE_SHOT_SOURCE").filter(|_| std::env::var_os("PAUSETAPE_SHOT").is_some()) {
            saved.source = Some(dir.into());
        }
        let first_run = saved.source.is_none();
        let mut app = App {
            active_playlist: (!saved.playlists.is_empty()).then_some(0),
            saved,
            tracks: Vec::new(),
            index: Index::default(),
            tab: Tab::Library,
            welcome_open: first_run,
            lyrics: HashMap::new(),
            lyrics_tx: None,
            details_lyrics: false,
            lyrics_song: None,
            lyrics_follow: true,
            filter: String::new(),
            duplicates_only: false,
            look_alikes_only: false,
            expanded: HashSet::new(),
            artist_links_open: HashSet::new(),
            selected: BTreeSet::new(),
            select_anchor: None,
            tag_edit: None,
            cover_misses: Vec::new(),
            album_sort: (SortBy::Artist, false),
            selected_albums: BTreeSet::new(),
            album_anchor: None,
            take_over_open: false,
            adding: None,
            source_missing: false,
            sync_confirm: None,
            pattern_draft: None,
            player_test_open: false,
            test_card: None,
            busy: None,
            notices: Vec::new(),
            undo_stack: Vec::new(),
            undo_notice: None,
            confirm: None,
            pending_add: None,
            focus_search: false,
            scroll_to_song: None,
            album_scroll_to: None,
            album_rows_shown: 0..0,
            tx,
            rx,
            mb: None,
            mb_tx,
            mb_rx,
            artist_info: HashMap::new(),
            artists_loaded: false,
            genre_index: GenreIndex::default(),
            genres_by_size: false,
            lookups: HashMap::new(),
            tidy_plan: None,
            in_place: 0,
            write_preview: None,
            card_survey: None,
            card_found: None,
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
            card_changes_only: false,
            card_setup_open: false,
            card_expanded: HashSet::new(),
            card_renaming: None,
            audio,
            library_generation: 0,
            library_read: None,
            queue: Vec::new(),
            qpos: 0,
            envelope: None,
            cover: None,
            detail_cover: None,
            details_sheet: false,
            details_beside: false,
            system_reduce_motion: crate::platform::reduce_motion(),
            renaming: None,
            db: Db::open_default().ok(),
            ctx,
            #[cfg(debug_assertions)]
            shooter: crate::ui::screenshots::Shooter::from_env(),
        };
        app.apply_theme();
        app.ctx.set_zoom_factor(app.saved.text_size.clamp(TEXT_SIZES.0, TEXT_SIZES.1));
        app.load();
        app
    }

    pub fn apply_theme(&self) {
        self.ctx.set_theme(match self.saved.theme {
            ThemePref::System => egui::ThemePreference::System,
            ThemePref::Light => egui::ThemePreference::Light,
            ThemePref::Dark => egui::ThemePreference::Dark,
        });
        crate::ui::theme::set_accent(&self.ctx, self.saved.accent);
    }

    /// Keep the errors of a card operation for the card tab, saved to a file too, and return the
    /// note for its notice ("" if there were none).
    pub fn report_problems(&mut self, what: &str, errors: Vec<String>) -> String {
        if errors.is_empty() {
            return String::new();
        }
        let note = format!(" · {} {}: see the list at the top of the page", errors.len(), if errors.len() == 1 { "problem" } else { "problems" });
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
        // The text size as ⌘+ and ⌘− left it, kept for the next start.
        self.saved.text_size = ui.ctx().zoom_factor();
        crate::ui::motion::set_reduced(ui.ctx(), self.saved.reduce_motion || self.system_reduce_motion);
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
