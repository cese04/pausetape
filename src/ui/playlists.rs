//! The playlists tab: make, reorder and play playlists (they reach the card as folders of
//! numbered copies).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{Align, FontId, Layout, RichText, ScrollArea, Ui};

use super::text;
use super::theme::ink;
use super::icons::Icon;
use super::widgets::{self, caps, icon_button, icon_link, muted};
use crate::app::App;
use crate::card::playlist::Playlist;
use crate::library::Track;
use super::layout::{S1, S2, S3, row};

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let device = app.player();
    muted(
        ui,
        "The Echo Mini has no playlists and FAT/exFAT cards can't hold symbolic links, so each playlist is exported as a folder of numbered copies. Hold Play on the folder to play it in order. An .m3u8 is written alongside for future firmware.",
    );
    if device.browses_by_tags() {
        ui.label(RichText::new(playlist_copies_warning(device)).size(13.0).color(ink.warn));
    }
    ui.add_space(S1);
    // Playlists hold track ids: a song inside an album file has its own ("…/Album.flac#03").
    let ids: Vec<PathBuf> = app.tracks.iter().map(Track::id).collect();
    let by_path: HashMap<&Path, &Track> = ids.iter().map(PathBuf::as_path).zip(&app.tracks).collect();
    let by_index: HashMap<&Path, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_path(), i)).collect();
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
            ui.add_space(S1);
            if icon_link(ui, Icon::Add, "new playlist").clicked() {
                app.saved.playlists.push(Playlist { name: format!("Playlist {}", app.saved.playlists.len() + 1), tracks: Vec::new() });
                select = Some(app.saved.playlists.len() - 1);
            }
            ui.add_space(S2);
            let total: u64 = app.saved.playlists.iter().map(|p| p.bytes(&by_path)).sum();
            muted(ui, format!("copies need {}", text::bytes(total)));
            ui.label(RichText::new("choose which playlists go on a card in the card tab").size(12.0).color(ink.muted));
        });
        ui.add_space(S3);
        ui.vertical(|ui| {
            let Some(pi) = app.active_playlist.filter(|&i| i < app.saved.playlists.len()) else {
                muted(ui, "Create a playlist, then use the + at the end of any track row in the library.");
                return;
            };
            let mut delete = false;
            let old_name = app.saved.playlists[pi].name.clone();
            row(ui, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut app.saved.playlists[pi].name).align(egui::Align2::LEFT_CENTER).font(FontId::proportional(20.0)).text_color(ink.text).frame(egui::Frame::NONE).desired_width(320.0));
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
                    if icon_link(ui, Icon::Trash, "delete").clicked() {
                        delete = true;
                    }
                    if icon_link(ui, Icon::Play, "play").clicked() {
                        play = Some(0);
                    }
                });
            });
            let pl = &mut app.saved.playlists[pi];
            let dur: Duration = pl.tracks.iter().filter_map(|p| by_path.get(p.as_path())).map(|t| t.duration).sum();
            muted(ui, format!("{} tracks · {} · {}", pl.tracks.len(), text::duration(dur), text::bytes(pl.bytes(&by_path))));
            ui.add_space(S1);
            let mut op: Option<(usize, i32)> = None;
            ScrollArea::vertical().auto_shrink(false).max_height(ui.available_height() - 40.0).show(ui, |ui| {
                for (n, path) in pl.tracks.iter().enumerate() {
                    row(ui, |ui| {
                        ui.add_sized([28.0, 18.0], egui::Label::new(RichText::new(format!("{}", n + 1)).color(ink.muted)));
                        match by_path.get(path.as_path()) {
                            Some(t) => {
                                ui.label(t.display_title());
                                muted(ui, t.artist.as_deref().unwrap_or(""));
                                if let Some(note) = text::format_note(t, device) {
                                    ui.label(RichText::new(text::format_badge(t, device)).size(12.0).color(ink.warn)).on_hover_text(note);
                                }
                            }
                            None => {
                                ui.label(RichText::new(path.display().to_string()).color(ink.warn)).on_hover_text("file not found in library");
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if icon_button(ui, Icon::Close).on_hover_text("remove from the playlist").clicked() {
                                op = Some((n, 0));
                            }
                            if icon_button(ui, Icon::Down).on_hover_text("move down").clicked() {
                                op = Some((n, 1));
                            }
                            if icon_button(ui, Icon::Up).on_hover_text("move up").clicked() {
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


/// A player that lists songs from their tags also lists a playlist's copies.
pub fn playlist_copies_warning(device: crate::card::Player) -> String {
    format!("{} lists songs by their tags, so each copy also shows up as a second song under its artist.", device.name())
}
