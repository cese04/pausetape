//! The playlists tab: make, reorder and play playlists (they reach the card as folders of
//! numbered copies).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{Align, FontId, Layout, RichText, ScrollArea, Ui};

use super::text;
use super::theme::ink;
use super::icons::Icon;
use super::widgets::{icon_button, icon_link, muted};
use crate::app::App;
use crate::library::Track;
use super::layout::{S1, S2, row, font, list_rows};

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let device = app.player();
    if device.browses_by_tags() {
        ui.label(RichText::new(playlist_copies_warning(device)).size(font::UI).color(ink.warn));
    }
    ui.add_space(S1);
    // Playlists hold track ids: a song inside an album file has its own ("…/Album.flac#03").
    let ids: Vec<PathBuf> = app.tracks.iter().map(Track::id).collect();
    let by_path: HashMap<&Path, &Track> = ids.iter().map(PathBuf::as_path).zip(&app.tracks).collect();
    let by_index: HashMap<&Path, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_path(), i)).collect();
    let mut play = None;
    // What was asked, done once drawing is over (the rows borrow the library's songs).
    let mut todo: Vec<Do> = Vec::new();
    let mut actions = Vec::new();
    // The playlists are in the sidebar; this is the one open.
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            let Some(pi) = app.active_playlist.filter(|&i| i < app.saved.playlists.len()) else {
                muted(ui, "No playlist open. Make one with New playlist in the sidebar, or drag songs or albums onto it.");
                return;
            };
            row(ui, |ui| {
                // The name is edited as a draft and taken when the field is left (Enter, Tab, a
                // click elsewhere): card setups refer to playlists by name, so it's checked then.
                let draft_id = egui::Id::unique(("playlist name", pi));
                let mut draft = ui.data_mut(|d| d.get_temp::<String>(draft_id)).unwrap_or_else(|| app.saved.playlists[pi].name.clone());
                let hand = FontId::new(font::HEADING, super::theme::hand());
                let name_w = (ui.painter().layout_no_wrap(draft.clone(), hand.clone(), ink.text).size().x + S2).clamp(80.0, 320.0);
                let r = ui.add(egui::TextEdit::singleline(&mut draft).align(egui::Align2::LEFT_CENTER).font(hand).text_color(ink.text).frame(egui::Frame::NONE).desired_width(name_w)).on_hover_text("click to rename");
                if r.has_focus() {
                    ui.data_mut(|d| d.insert_temp(draft_id, draft));
                } else {
                    if r.lost_focus() {
                        todo.push(Do::Rename(pi, draft));
                    }
                    ui.data_mut(|d| d.remove::<String>(draft_id));
                }
                if !r.has_focus() && icon_button(ui, Icon::Rename, "Rename").clicked() {
                    r.request_focus();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_link(ui, Icon::Trash, "Delete").on_hover_text("undo brings it back").clicked() {
                        todo.push(Do::Delete(pi));
                    }
                    if icon_link(ui, Icon::Play, "Play").clicked() {
                        play = Some(0);
                    }
                });
            });
            let pl = &app.saved.playlists[pi];
            let dur: Duration = pl.tracks.iter().filter_map(|p| by_path.get(p.as_path())).map(|t| t.duration).sum();
            let how = "The Echo Mini has no playlists and FAT/exFAT cards can't hold symbolic links, so each playlist is written to the card as a folder of numbered copies. Hold Play on the folder to play it in order. An .m3u8 is written alongside for future firmware. Which playlists go on a card: the card's Setup.";
            muted(ui, format!("{} tracks · {} · on the card, a folder of copies ({})", pl.tracks.len(), text::duration(dur), text::bytes(pl.bytes(&by_path)))).on_hover_text(how);
            ui.add_space(S1);
            ScrollArea::vertical().auto_shrink(false).max_height(ui.available_height() - 40.0).show(ui, |ui| {
                let compact = app.saved.compact;
                list_rows(ui, compact, |ui| {
                    for (n, path) in pl.tracks.iter().enumerate() {
                        let title = by_path.get(path.as_path()).map(|t| t.display_title()).unwrap_or_else(|| "a missing song".into());
                        let line = row(ui, |ui| {
                            // A handle to drag the song to another place in the list.
                            let (handle, h) = ui.allocate_exact_size(egui::vec2(S2, 18.0), egui::Sense::drag());
                            let dot = if h.hovered() || h.dragged() { ink.text } else { ink.faint };
                            for (dx, dy) in [(0.0, -4.0), (0.0, 0.0), (0.0, 4.0), (5.0, -4.0), (5.0, 0.0), (5.0, 4.0)] {
                                ui.painter().circle_filled(handle.center() + egui::vec2(dx - 2.5, dy), 1.2, dot);
                            }
                            h.on_hover_text("drag to move").dnd_set_drag_payload(super::drag::PlaylistSong(pi, n));
                            ui.add_sized([28.0, 18.0], egui::Label::new(RichText::new(format!("{}", n + 1)).color(ink.muted)));
                            match by_path.get(path.as_path()) {
                                Some(t) => {
                                    let r = ui.add(egui::Label::new(t.display_title()).sense(egui::Sense::click()));
                                    if let Some(&ti) = by_index.get(path.as_path()) {
                                        r.context_menu(|ui| {
                                            super::menu::music(app, ui, super::menu::Of::Songs(vec![ti]), &mut actions, |ui, _| {
                                                ui.separator();
                                                if icon_link(ui, Icon::Close, "Remove from the playlist").clicked() {
                                                    todo.push(Do::Remove(pi, n, format!("taking “{title}” out of “{}”", pl.name)));
                                                    ui.close();
                                                }
                                            });
                                        });
                                    }
                                    muted(ui, t.artist.as_deref().unwrap_or(""));
                                    if let Some(note) = text::format_note(t, device) {
                                        ui.label(RichText::new(text::format_badge(t, device)).size(font::META).color(ink.warn)).on_hover_text(note);
                                    }
                                }
                                None => {
                                    ui.label(RichText::new(path.display().to_string()).color(ink.warn)).on_hover_text("file not found in library");
                                }
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(ui, Icon::Close, "Remove from the playlist").clicked() {
                                    todo.push(Do::Remove(pi, n, format!("taking “{title}” out of “{}”", pl.name)));
                                }
                                if n + 1 < pl.tracks.len() && icon_button(ui, Icon::Down, "Move down").clicked() {
                                    todo.push(Do::Swap(pi, n, n + 1, format!("moving “{title}” down")));
                                }
                                if n > 0 && icon_button(ui, Icon::Up, "Move up").clicked() {
                                    todo.push(Do::Swap(pi, n, n - 1, format!("moving “{title}” up")));
                                }
                            });
                        });
                        // Another of this list's songs held over the row: a line where it would go
                        // (above when it comes from below, and the other way); let go, it goes there.
                        let r = line.response;
                        if let Some(held) = r.dnd_hover_payload::<super::drag::PlaylistSong>()
                            && held.0 == pi
                            && held.1 != n
                        {
                            let y = if held.1 > n { r.rect.top() } else { r.rect.bottom() };
                            ui.painter().hline(r.rect.x_range(), y, egui::Stroke::new(2.0, ink.accent));
                        }
                        if let Some(held) = r.dnd_release_payload::<super::drag::PlaylistSong>()
                            && held.0 == pi
                            && held.1 != n
                        {
                            todo.push(Do::Move(pi, held.1, n, "moving a song".into()));
                        }
                    }
                });
            });
        });
    });
    super::actions::apply(app, actions);
    for d in todo {
        match d {
            Do::Rename(i, name) => {
                app.rename_playlist(i, &name);
            }
            Do::Delete(i) => app.delete_playlist(i),
            Do::Remove(i, n, what) => {
                app.remember_playlists(what);
                app.saved.playlists[i].tracks.remove(n);
            }
            Do::Swap(i, a, b, what) => {
                app.remember_playlists(what);
                app.saved.playlists[i].tracks.swap(a, b);
            }
            Do::Move(i, from, to, what) => {
                app.remember_playlists(what);
                let tracks = &mut app.saved.playlists[i].tracks;
                let song = tracks.remove(from);
                tracks.insert(to.min(tracks.len()), song);
            }
        }
    }
    if let Some(start) = play
        && let Some(pl) = app.active_playlist.and_then(|i| app.saved.playlists.get(i))
    {
        let q: Vec<usize> = pl.tracks.iter().filter_map(|p| by_index.get(p.as_path()).copied()).collect();
        app.play(q, start);
    }
}


/// A change to the playlists, asked for while drawing them.
enum Do {
    Rename(usize, String),
    Delete(usize),
    /// Playlist, song, and how undo will name it.
    Remove(usize, usize, String),
    Swap(usize, usize, usize, String),
    /// Playlist, from, to: the song taken out and put back at `to`.
    Move(usize, usize, usize, String),
}

/// A player that lists songs from their tags also lists a playlist's copies.
pub fn playlist_copies_warning(device: crate::card::Player) -> String {
    format!("{} lists songs by their tags, so each copy also shows up as a second song under its artist.", device.name())
}
