//! Application state and the glue between background work and the UI. Rendering lives in
//! `crate::ui`; this module owns the data, starts background work and folds its results back in.
//!
//! The `impl App` is split by topic: `music` (scanning, artists, MusicBrainz), `writes` (card
//! writes, reorganising, playlists), `playback`, and `events` (the per-frame message pump).

mod events;
mod music;
mod playback;
mod writes;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};
use serde::{Deserialize, Serialize};

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
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            source: None,
            writes: vec![WriteProfile::default()],
            active_write: 0,
            // Tidying keeps every format and leaves Finder files alone: it's the user's own disk.
            tidy: Options { skip_unplayable: false, clean_mac_files: false, ..Options::default() },
            mb_contact: String::new(),
            prefs: ArtistPrefs::default(),
            playlists: Vec::new(),
            volume: 0.8,
            theme: ThemePref::System,
            format_note_collapsed: false,
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
    /// (track index, new size, new mtime) for files whose tags were written.
    TagsSaved(Vec<(usize, u64, u64)>, Vec<String>),
    /// Loudness of a track, by track id.
    Envelope(PathBuf, Vec<f32>),
    /// Cover for a track path, and the image file it came from if it isn't embedded.
    Cover(PathBuf, Option<egui::ColorImage>, Option<PathBuf>),
}

pub struct Busy {
    pub label: String,
    pub done: usize,
    pub total: usize,
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
    /// Open folder/card dialog.
    pub picker: Option<crate::ui::folders::Picker>,
    /// Library A–Z rail: jump to the first artist under this letter on the next frame.
    pub jump_to: Option<char>,
    pub volumes: Vec<Volume>,
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
            picker: None,
            jump_to: None,
            volumes: card::volumes(),
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

fn plural_files(n: usize) -> String {
    if n == 1 { "1 file".into() } else { format!("{n} files") }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
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
