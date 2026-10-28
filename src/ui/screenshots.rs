//! Debug builds only: `NEATLIBRARY_SHOT=<dir>` stages a demo (expand, play, look up, playlist),
//! saves a screenshot of every tab as PPM, then quits. `NEATLIBRARY_SHOT_ARTIST` (default
//! Radiohead) chooses the album played and looked up. Handy for checking the look headlessly.
//! The demo changes playlists and the active write in memory only: `App::save` skips saving
//! while it runs, so a tester's real settings are left alone.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::app::{App, Tab};
use crate::card::playlist::Playlist;

const TABS: [(Tab, &str); 8] = [
    (Tab::Folders, "folders"),
    (Tab::Folders, "picker"),
    (Tab::Folders, "cardpicker"),
    (Tab::Library, "library"),
    (Tab::Artists, "artists"),
    (Tab::Card, "card"),
    (Tab::Playlists, "playlists"),
    (Tab::Settings, "settings"),
];

pub struct Shooter {
    dir: PathBuf,
    step: usize,
    until: Option<Instant>,
}

impl Shooter {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("NEATLIBRARY_SHOT").map(|d| Shooter { dir: d.into(), step: 0, until: None })
    }

    pub fn tick(&mut self, app: &mut App, ctx: &egui::Context) {
        ctx.request_repaint();
        if app.tracks.is_empty() || app.busy.is_some() || self.until.is_some_and(|t| Instant::now() < t) {
            return;
        }
        if self.step == 0 {
            // NEATLIBRARY_SHOT_SIZE=720x560 checks narrow-window layouts.
            if let Some((w, h)) = std::env::var("NEATLIBRARY_SHOT_SIZE").ok().and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
            }) {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
            }
            for a in &app.index.artists {
                app.expanded.insert(crate::library::names::artist_key(&a.name));
            }
            // NEATLIBRARY_SHOT_ARTIST picks whose album plays and is looked up (preferring an
            // album stored as one file with a CUE sheet).
            let who = std::env::var("NEATLIBRARY_SHOT_ARTIST").unwrap_or_else(|_| "Radiohead".into());
            let found = app.index.artists.iter().enumerate().find(|(_, a)| a.name.eq_ignore_ascii_case(&who));
            if let Some((ai, a)) = found {
                let al = a.albums.iter().position(|al| al.tracks.iter().any(|&i| app.tracks[i].cue.is_some())).unwrap_or(0);
                let (queue, cluster) = (a.albums[al].tracks.clone(), a.cluster);
                app.play(queue, 1);
                app.lookup_album(ai, al);
                app.lookup_artist(cluster);
            }
            let some: Vec<PathBuf> = app.tracks.iter().take(5).map(|t| t.id()).collect();
            app.saved.playlists = vec![Playlist { name: "Road Trip".into(), tracks: some }];
            app.active_playlist = Some(0);
            app.preview_tidy();
            app.filter = std::env::var("NEATLIBRARY_SHOT_FILTER").unwrap_or_default();
            app.card_filter = app.filter.clone();
            for a in &app.index.artists {
                app.card_expanded.insert(app.index.clusters[a.cluster].key.clone());
            }
            app.write_mut().playlists.insert("Road Trip".into());
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
