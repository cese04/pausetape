//! The playlists tab: make, reorder and play playlists (they reach the card as folders of
//! numbered copies).

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use egui::{Align, FontId, Layout, RichText, ScrollArea, Ui};

use super::text;
use super::theme::ink;
use super::widgets::{self, caps, link, muted};
use crate::app::App;
use crate::card::playlist::Playlist;
use crate::library::Track;

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    muted(
        ui,
        "RECHO has no playlists and FAT/exFAT cards can't hold symbolic links, so each playlist is exported as a folder of numbered copies. Hold Play on the folder to play it in order. An .m3u8 is written alongside for future firmware.",
    );
    ui.add_space(8.0);
    let by_path: HashMap<&Path, &Track> = app.tracks.iter().map(|t| (t.path.as_path(), t)).collect();
    let by_index: HashMap<&Path, usize> = app.tracks.iter().enumerate().map(|(i, t)| (t.path.as_path(), i)).collect();
    let mut select = None;
    let mut play = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(200.0);
            caps(ui, "playlists");
            for (i, p) in app.saved.playlists.iter().enumerate() {
                if widgets::tab(ui, &p.name, app.active_playlist == Some(i)).clicked() {
                    select = Some(i);
                }
            }
            ui.add_space(6.0);
            if link(ui, "new playlist").clicked() {
                app.saved.playlists.push(Playlist { name: format!("Playlist {}", app.saved.playlists.len() + 1), tracks: Vec::new() });
                select = Some(app.saved.playlists.len() - 1);
            }
            ui.add_space(12.0);
            let total: u64 = app.saved.playlists.iter().map(|p| p.bytes(&by_path)).sum();
            muted(ui, format!("copies need {}", text::bytes(total)));
            ui.label(RichText::new("choose which playlists go on a card in the card tab").size(12.0).color(ink.muted));
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            let Some(pi) = app.active_playlist.filter(|&i| i < app.saved.playlists.len()) else {
                muted(ui, "Create a playlist, then use the + at the end of any track row in the library.");
                return;
            };
            let mut delete = false;
            let old_name = app.saved.playlists[pi].name.clone();
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut app.saved.playlists[pi].name).font(FontId::proportional(20.0)).text_color(ink.text).frame(egui::Frame::NONE).desired_width(320.0));
                // Writes refer to playlists by name; follow the rename.
                if r.changed() {
                    let new = app.saved.playlists[pi].name.clone();
                    for w in &mut app.saved.writes {
                        if w.playlists.remove(&old_name) {
                            w.playlists.insert(new.clone());
                        }
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if link(ui, "delete").clicked() {
                        delete = true;
                    }
                    if link(ui, "play").clicked() {
                        play = Some(0);
                    }
                });
            });
            let pl = &mut app.saved.playlists[pi];
            let dur: Duration = pl.tracks.iter().filter_map(|p| by_path.get(p.as_path())).map(|t| t.duration).sum();
            muted(ui, format!("{} tracks · {} · {}", pl.tracks.len(), text::duration(dur), text::bytes(pl.bytes(&by_path))));
            ui.add_space(6.0);
            let mut op: Option<(usize, i32)> = None;
            ScrollArea::vertical().auto_shrink(false).max_height(ui.available_height() - 40.0).show(ui, |ui| {
                for (n, path) in pl.tracks.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized([28.0, 18.0], egui::Label::new(RichText::new(format!("{}", n + 1)).color(ink.muted)));
                        match by_path.get(path.as_path()) {
                            Some(t) => {
                                ui.label(t.display_title());
                                muted(ui, t.artist.as_deref().unwrap_or(""));
                                if let Some(note) = text::format_note(t) {
                                    ui.label(RichText::new(text::format_badge(t)).size(12.0).color(ink.accent)).on_hover_text(note);
                                }
                            }
                            None => {
                                ui.label(RichText::new(path.display().to_string()).color(ink.accent)).on_hover_text("file not found in library");
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if link(ui, "×").clicked() {
                                op = Some((n, 0));
                            }
                            if link(ui, "↓").clicked() {
                                op = Some((n, 1));
                            }
                            if link(ui, "↑").clicked() {
                                op = Some((n, -1));
                            }
                        });
                    });
                }
            });
            match op {
                Some((n, 0)) => {
                    pl.tracks.remove(n);
                }
                Some((n, d)) => {
                    let m = n as i32 + d;
                    if m >= 0 && (m as usize) < pl.tracks.len() {
                        pl.tracks.swap(n, m as usize);
                    }
                }
                None => {}
            }
            if delete {
                app.saved.playlists.remove(pi);
                app.active_playlist = app.saved.playlists.len().checked_sub(1);
            }
        });
    });
    if let Some(i) = select {
        app.active_playlist = Some(i);
    }
    if let Some(start) = play
        && let Some(pl) = app.active_playlist.and_then(|i| app.saved.playlists.get(i))
    {
        let q: Vec<usize> = pl.tracks.iter().filter_map(|p| by_index.get(p.as_path()).copied()).collect();
        app.play(q, start);
    }
}

