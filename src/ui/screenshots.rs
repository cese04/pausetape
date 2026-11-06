//! Debug builds only: `PAUSETAPE_SHOT=<dir>` stages a demo (expand, play, look up, playlist),
//! saves a screenshot of every tab as PPM, then quits. `PAUSETAPE_SHOT_ARTIST` (default
//! Radiohead) chooses the album played and looked up; more switches below. Handy for checking
//! the look headlessly.
//! The demo changes playlists and the active write in memory only: `App::save` skips saving
//! while it runs, so a tester's real settings are left alone.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::app::{App, Tab};
use crate::card::playlist::Playlist;

const TABS: [(Tab, &str); 16] = [
    (Tab::Library, "folders"),
    (Tab::Library, "picker"),
    (Tab::Library, "cardpicker"),
    (Tab::Library, "library"),
    (Tab::Library, "tageditor"),
    (Tab::Library, "job"),
    (Tab::Library, "albums"),
    (Tab::Library, "covers"),
    (Tab::Card, "card"),
    (Tab::Card, "cardowner"),
    (Tab::Card, "syncconfirm"),
    (Tab::Card, "takeover"),
    (Tab::Card, "playertest"),
    (Tab::Playlists, "playlists"),
    (Tab::Genres, "genres"),
    (Tab::Settings, "settings"),
];

pub struct Shooter {
    dir: PathBuf,
    step: usize,
    until: Option<Instant>,
    /// PAUSETAPE_SHOT_SURVEY: check the card's own music once the comparison is in (reads only).
    survey: bool,
    /// The app looks for cards plugged in only once the demo is set up (PAUSETAPE_SHOT_WATCH).
    pub watch: bool,
}

impl Shooter {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("PAUSETAPE_SHOT").map(|d| Shooter { dir: d.into(), step: 0, until: None, survey: std::env::var_os("PAUSETAPE_SHOT_SURVEY").is_some(), watch: false })
    }

    pub fn tick(&mut self, app: &mut App, ctx: &egui::Context) {
        ctx.request_repaint();
        // The "job" shot shows a pretend write, so it doesn't wait for that one.
        let pretend = app.busy.as_ref().is_some_and(|b| b.stop.is_some()) && self.step / 2 < TABS.len() && TABS[self.step.saturating_sub(1) / 2].1 == "job";
        // PAUSETAPE_SHOT_SOURCE=<missing folder>: the library stays empty; go on once that's known.
        let waiting = app.tracks.is_empty() && !app.source_missing;
        if waiting || (app.busy.is_some() && !pretend) || self.until.is_some_and(|t| Instant::now() < t) {
            return;
        }
        if self.survey && app.write_preview.as_ref().is_some_and(|(_, p)| p.foreign > 0) {
            self.survey = false;
            app.survey_card();
            return;
        }
        if self.step == 0 {
            // PAUSETAPE_SHOT_SIZE=720x560 checks narrow-window layouts.
            if let Some((w, h)) = std::env::var("PAUSETAPE_SHOT_SIZE").ok().and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
            }) {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
            }
            // PAUSETAPE_SHOT_THEME=light|dark, whatever the saved choice.
            match std::env::var("PAUSETAPE_SHOT_THEME").as_deref() {
                Ok("light") => ctx.set_theme(egui::ThemePreference::Light),
                Ok("dark") => ctx.set_theme(egui::ThemePreference::Dark),
                _ => {}
            }
            // PAUSETAPE_SHOT_ACCENT=teal (any name from the settings tab).
            if let Some(a) = std::env::var("PAUSETAPE_SHOT_ACCENT").ok().and_then(|n| crate::ui::theme::Accent::ALL.into_iter().find(|a| a.name() == n)) {
                app.saved.accent = a;
                crate::ui::theme::set_accent(ctx, a);
            }
            for a in &app.index.artists {
                app.expanded.insert(crate::library::names::artist_key(&a.name));
            }
            // PAUSETAPE_SHOT_ARTIST picks whose album plays and is looked up (preferring an
            // album stored as one file with a CUE sheet).
            let who = std::env::var("PAUSETAPE_SHOT_ARTIST").unwrap_or_else(|_| "Radiohead".into());
            let found = app.index.artists.iter().enumerate().find(|(_, a)| a.name.eq_ignore_ascii_case(&who));
            if let Some((ai, a)) = found {
                let al = a.albums.iter().position(|al| al.tracks.iter().any(|&i| app.tracks[i].cue.is_some())).unwrap_or(0);
                let (queue, cluster) = (a.albums[al].tracks.clone(), a.cluster);
                app.play(queue, 1);
                app.lookup_album(ai, al);
                app.ask_artist_info(cluster, None);
            }
            let some: Vec<PathBuf> = app.tracks.iter().take(5).map(|t| t.id()).collect();
            app.saved.playlists = vec![Playlist { name: "Road Trip".into(), tracks: some }];
            app.active_playlist = Some(0);
            app.preview_tidy();
            app.filter = std::env::var("PAUSETAPE_SHOT_FILTER").unwrap_or_default();
            // PAUSETAPE_SHOT_DUPLICATES=1 opens the library on the duplicate copies.
            app.duplicates_only = std::env::var_os("PAUSETAPE_SHOT_DUPLICATES").is_some();
            app.card_filter = app.filter.clone();
            for a in &app.index.artists {
                app.card_expanded.insert(app.index.clusters[a.cluster].key.clone());
            }
            app.write_mut().playlists.insert("Road Trip".into());
            // PAUSETAPE_SHOT_ON_CARD=<text>: pretend, in memory, that the write put the albums
            // whose title contains <text> (all but one song of the first) on the card.
            if let Ok(what) = std::env::var("PAUSETAPE_SHOT_ON_CARD") {
                let plan = app.write().plan(&app.tracks, &app.index);
                let mut first = true;
                let mut written = Vec::new();
                for a in &app.index.artists {
                    for al in a.albums.iter().filter(|al| al.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&what.to_lowercase()))) {
                        let skip_one = std::mem::take(&mut first);
                        for p in plan.iter().filter(|p| al.tracks.contains(&p.track)).skip(skip_one as usize) {
                            written.push(crate::card::Written { rel: p.rel.clone(), size: 0, source: None });
                        }
                    }
                }
                app.write_mut().manifest = written;
                app.refresh_on_card();
            }
            // PAUSETAPE_SHOT_TOGGLE=<text>: send albums whose title contains <text> to the card,
            // or take them off, in memory, to show the marks of changes waiting for Sync.
            if let Ok(what) = std::env::var("PAUSETAPE_SHOT_TOGGLE") {
                let mut toggle = Vec::new();
                for (ai, a) in app.index.artists.iter().enumerate() {
                    let ck = &app.index.clusters[a.cluster].key;
                    for (bi, al) in a.albums.iter().enumerate().filter(|(_, al)| al.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&what.to_lowercase()))) {
                        toggle.push((ai, bi, !app.write().custom.includes(ck, al, &a.albums)));
                    }
                }
                for (ai, bi, on) in toggle {
                    app.card_album(ai, bi, on);
                }
            }
            // PAUSETAPE_SHOT_DEVICE=stock: write for the Echo Mini's stock firmware instead of RECHO.
            if std::env::var("PAUSETAPE_SHOT_DEVICE").is_ok_and(|d| d == "stock") {
                app.write_mut().options.device = crate::card::Device::EchoMiniStock;
            }
            // PAUSETAPE_SHOT_PATTERN=<pattern>: name the card's songs by it.
            if let Ok(p) = std::env::var("PAUSETAPE_SHOT_PATTERN") {
                app.write_mut().options.pattern = Some(p);
                app.refresh_on_card();
            }
            // PAUSETAPE_SHOT_QUEUE=1: two songs of another album to play next, and the queue open.
            if std::env::var_os("PAUSETAPE_SHOT_QUEUE").is_some()
                && let Some(other) = app.index.artists.iter().flat_map(|a| &a.albums).find(|al| !al.tracks.iter().any(|t| app.queue.contains(t)))
            {
                let next: Vec<usize> = other.tracks.iter().take(2).copied().collect();
                app.play_next(&next);
            }
            // PAUSETAPE_SHOT_CARD_SETUP=1: the card's view on its setup rather than its albums.
            app.card_setup_open = std::env::var_os("PAUSETAPE_SHOT_CARD_SETUP").is_some();
            // PAUSETAPE_SHOT_SELECT=1: three songs of the album that plays chosen for editing.
            if std::env::var_os("PAUSETAPE_SHOT_SELECT").is_some() {
                app.selected = app.queue.iter().take(3).copied().collect();
            }
            // PAUSETAPE_SHOT_CARD=<folder>: the card this write goes to (only read; the website's
            // shots use an empty FAT32 disk image).
            if let Some(card) = std::env::var_os("PAUSETAPE_SHOT_CARD") {
                app.write_mut().dest = Some(card.into());
            }
            // PAUSETAPE_SHOT_EMPTY=1: nothing chosen, as in a new setup for a card.
            if std::env::var_os("PAUSETAPE_SHOT_EMPTY").is_some() {
                app.write_mut().custom.chosen = Some(Default::default());
            }
            // PAUSETAPE_SHOT_ROOM=<GB>: pretend the card (an empty temporary folder) has this
            // much room, to show what the card tab offers when the choice doesn't fit.
            if let Some(gb) = std::env::var("PAUSETAPE_SHOT_ROOM").ok().and_then(|v| v.parse::<f64>().ok()) {
                let card = std::env::temp_dir().join("pausetape-shot-card");
                // Something copied by hand, for the take-over.
                let _ = std::fs::create_dir_all(card.join("Hand Copied"));
                let _ = std::fs::write(card.join("Hand Copied/01 - Song.mp3"), b"not really a song");
                app.write_mut().dest = Some(card);
                app.room_while_writing = Some((gb * 1e9) as u64);
            }
            // PAUSETAPE_SHOT_WATCH=<a mounted scratch volume>: label it for a second setup,
            // "Road trip", that shares the first one's destination, then look for cards: the
            // volume should bring "Road trip" up. Writes only the label there.
            if let Some(vol) = std::env::var_os("PAUSETAPE_SHOT_WATCH").map(PathBuf::from) {
                app.write_mut().dest = Some(vol.clone());
                app.new_write(true);
                app.write_mut().name = "Road trip".into();
                let _ = crate::card::Label::of(app.write()).save(&vol);
                app.select_write(0);
                self.watch = true;
            }
            app.preview_write();
        }
        let (n, half) = (self.step / 2, self.step % 2);
        if n >= TABS.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if half == 0 {
            app.tab = TABS[n].0;
            app.picker = match TABS[n].1 {
                "picker" => Some(crate::ui::folders::Picker::Source(crate::ui::navigator::Navigator::new(app.saved.source.as_deref().and_then(|p| p.parent())))),
                "cardpicker" => Some(crate::ui::folders::Picker::Card { browse: None, show_all: true }),
                _ => None,
            };
            let filter = std::env::var("PAUSETAPE_SHOT_FILTER").unwrap_or_default();
            app.saved.album_table = TABS[n].1 == "albums";
            app.take_over_open = TABS[n].1 == "takeover";
            app.player_test_open = TABS[n].1 == "playertest";
            if TABS[n].1 == "playertest" {
                // Some answers, in memory, to show what they change.
                use crate::card::Sample;
                for (s, a) in [(Sample::Flac, true), (Sample::Aac, false), (Sample::Alac, true), (Sample::Flac384, false), (Sample::Cover, true)] {
                    app.set_tested(s, Some(a));
                }
            }
            // The first-run welcome, and the folder dialogs over it.
            app.welcome_open = matches!(TABS[n].1, "folders" | "picker" | "cardpicker");
            app.sync_confirm = (TABS[n].1 == "syncconfirm").then(|| (app.write().id.clone(), 4100, 4412));
            if TABS[n].1 == "cardowner" {
                // Another setup's card where this one writes: the note, and no Sync.
                if app.saved.writes.len() < 2 {
                    app.new_write(true);
                    app.write_mut().name = "Road trip".into();
                }
                let other = crate::card::Label::of(&app.saved.writes[1]);
                app.select_write(0);
                app.dest_label.get_or_insert(other);
            }
            // PAUSETAPE_SHOT_QUIET=1: no notice above the page (the scan's "40 songs…").
            if std::env::var_os("PAUSETAPE_SHOT_QUIET").is_some() {
                app.notices.clear();
            }
            app.filter = if TABS[n].1 == "covers" { "cover:no".into() } else { filter };
            if TABS[n].1 == "albums" {
                app.selected_albums = app.album_rows(&app.query()).iter().skip(2).take(2).map(|r| r.id.clone()).collect();
            }
            match TABS[n].1 {
                "tageditor" => app.edit_tags(app.queue.clone()),
                "job" => {
                    app.tag_edit = None;
                    app.busy = Some(crate::app::Busy::pretend("writing “Echo Mini”", 1204, 2466, 300));
                }
                "takeover" => {
                    app.tag_edit = None;
                    app.busy = None;
                    // The card's own label again, not the one the "cardowner" shot pretended, and
                    // the comparison (and check) that switching setups since has dropped.
                    app.refresh_label();
                    app.preview_write();
                    self.survey = std::env::var_os("PAUSETAPE_SHOT_SURVEY").is_some();
                }
                _ => {
                    app.tag_edit = None;
                    app.busy = None;
                }
            }
            // First tab waits for MusicBrainz and the waveform.
            self.until = Some(Instant::now() + Duration::from_millis(if n == 0 { 15000 } else { 600 }));
        } else {
            let path = self.dir.join(format!("{}.ppm", TABS[n].1));
            ctx.request_screenshot(move |img| {
                let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
                for p in &img.pixels {
                    out.extend_from_slice(&[p.r(), p.g(), p.b()]);
                }
                let _ = std::fs::write(path, out);
            });
            self.until = Some(Instant::now() + Duration::from_millis(400));
        }
        self.step += 1;
    }
}
