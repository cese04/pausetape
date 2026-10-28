//! Card tab: saved writes. Each is a named layout for a card: what goes on it, the names and
//! order it uses there, and a sync that only copies what changed.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Align, Layout, RichText, ScrollArea, Sense, Stroke, Ui, vec2};

use super::navigator::describe_space;
use super::text::{self, plural};
use super::theme::{self, ink};
use super::widgets::{self, caps, link, muted};
use crate::app::App;
use crate::card::{AlbumOrder, album_folder};
use crate::library::Track;

pub fn show(app: &mut App, ui: &mut Ui) {
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        writes_bar(app, ui);
        ui.add_space(14.0);
        if app.tracks.is_empty() {
            muted(ui, "Choose a source in the folders tab first.");
            return;
        }
        let sizes = Sizes::compute(app);
        capacity(app, ui, &sizes);
        ui.add_space(14.0);
        actions(app, ui);
        ui.add_space(14.0);
        selection(app, ui, &sizes);
        ui.add_space(14.0);
        playlists(app, ui);
        ui.add_space(14.0);
        layout_options(app, ui);
    });
}

fn writes_bar(app: &mut App, ui: &mut Ui) {
    let mut select = None;
    ui.horizontal_wrapped(|ui| {
        caps(ui, "writes");
        for (i, w) in app.saved.writes.iter().enumerate() {
            if widgets::tab(ui, &w.name, i == app.saved.active_write).clicked() {
                select = Some(i);
            }
            ui.add_space(4.0);
        }
        ui.add_space(12.0);
        if link(ui, "new").on_hover_text("a fresh layout with everything selected").clicked() {
            app.new_write(false);
        }
        if link(ui, "duplicate").on_hover_text("same choices, e.g. for a second card").clicked() {
            app.new_write(true);
        }
        if app.saved.writes.len() > 1 && link(ui, "delete").on_hover_text("forgets the layout; files on the card stay").clicked() {
            app.delete_write();
        }
    });
    if let Some(i) = select {
        app.select_write(i);
    }
    let ink = ink(ui);
    ui.horizontal(|ui| {
        let w = &mut app.saved.writes[app.saved.active_write];
        let old = w.name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut w.name).font(egui::FontId::proportional(22.0)).text_color(ink.text).frame(egui::Frame::NONE).desired_width(360.0));
        // Names identify writes for background replies; keep them unique and non-empty.
        if r.lost_focus() && (w.name.trim().is_empty() || app.saved.writes.iter().filter(|x| x.name == app.saved.writes[app.saved.active_write].name).count() > 1) {
            app.saved.writes[app.saved.active_write].name = old;
        }
    });
    let w = app.write();
    let last = match w.last_written {
        Some(t) => format!("last written {} · {} on the card from this write ({})", ago(t), plural(w.manifest.len(), "file"), text::bytes(w.manifest_bytes())),
        None => "never written".into(),
    };
    muted(ui, last);
    let shown = w.dest.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not chosen".into());
    let has_dest = w.dest.is_some();
    ui.horizontal(|ui| {
        caps(ui, "destination");
        if link(ui, if has_dest { "change" } else { "choose" }).clicked() {
            app.volumes = crate::card::volumes();
            app.picker = Some(crate::ui::folders::Picker::Card { browse: None, show_all: false });
        }
        ui.add(egui::Label::new(shown).truncate());
    });
}

fn ago(t: u64) -> String {
    let s = crate::card::now().saturating_sub(t);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

/// Bytes per artist (cluster key) and album id, counting only files this write would copy.
struct Sizes {
    artist: HashMap<String, u64>,
    album: HashMap<String, u64>,
    selected: u64,
    playlists: u64,
}

impl Sizes {
    fn compute(app: &App) -> Self {
        let w = app.write();
        let mut s = Sizes { artist: HashMap::new(), album: HashMap::new(), selected: 0, playlists: 0 };
        for a in &app.index.artists {
            let ck = &app.index.clusters[a.cluster].key;
            for al in &a.albums {
                let id = al.id(ck);
                let bytes: u64 = al
                    .tracks
                    .iter()
                    .map(|&i| &app.tracks[i])
                    .filter(|t| t.on_recho() || !w.options.skip_unplayable)
                    .map(|t| t.size)
                    .sum();
                *s.artist.entry(ck.clone()).or_default() += bytes;
                if !w.custom.excluded_artists.contains(ck) && !w.custom.excluded_albums.contains(&id) {
                    s.selected += bytes;
                }
                s.album.insert(id, bytes);
            }
        }
        let ids: Vec<PathBuf> = app.tracks.iter().map(Track::id).collect();
        let by_path = ids.iter().map(PathBuf::as_path).zip(&app.tracks).collect();
        s.playlists = crate::card::playlist::planned(&w.chosen_playlists(&app.saved.playlists), &by_path, &w.playlists_folder, w.options.skip_unplayable)
            .iter()
            .flat_map(|(_, copies, _)| copies.iter().map(|c| c.track.size))
            .sum();
        s
    }
}

fn capacity(app: &App, ui: &mut Ui, sizes: &Sizes) {
    let ink = ink(ui);
    let w = app.write();
    let need = sizes.selected + sizes.playlists;
    let info = w.dest.as_deref().and_then(crate::card::space);
    ui.horizontal(|ui| {
        caps(ui, "size");
        let mut s = format!("this write {}", text::bytes(need));
        if sizes.playlists > 0 {
            s.push_str(&format!(" (playlist copies {})", text::bytes(sizes.playlists)));
        }
        if let Some(i) = &info {
            s.push_str(&format!(" · card {}", describe_space(i)));
        }
        muted(ui, s);
    });
    let Some(info) = info else { return };
    // Space this write can use: what's free plus what its own earlier files occupy.
    let room = info.free + w.manifest_bytes();
    let width = ui.available_width().min(560.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 12.0), Sense::hover());
    let scale = width / room.max(need).max(1) as f32;
    let y = rect.center().y;
    ui.painter().hline(rect.left()..=rect.left() + room as f32 * scale, y, Stroke::new(1.0, ink.faint));
    let fits = need <= room;
    let used = rect.left() + need.min(room) as f32 * scale;
    ui.painter().hline(rect.left()..=used, y, Stroke::new(3.0, ink.text));
    if !fits {
        ui.painter().hline(used..=rect.left() + need as f32 * scale, y, Stroke::new(3.0, ink.accent));
        ui.label(RichText::new(format!("doesn't fit: {} over. Leave out some artists or albums below.", text::bytes(need - room))).color(ink.accent).size(13.0));
    }
}

fn actions(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let ready = app.write().dest.is_some() && app.saved.source.is_some();
    ui.horizontal(|ui| {
        if !ready {
            muted(ui, "choose a destination to compare or write");
            return;
        }
        if app.busy.is_none() && widgets::button(ui, "Compare with the card").on_hover_text("see what would be copied and removed; changes nothing").clicked() {
            app.preview_write();
        }
        if app.busy.is_none() && widgets::primary_button(ui, "Write to the card").clicked() {
            app.run_write();
        }
    });
    if let Some((_, p)) = &app.write_preview {
        let mut parts = vec![format!("{} to copy ({})", p.to_copy, text::bytes(p.copy_bytes)), format!("{} already there", p.unchanged)];
        if !p.to_remove.is_empty() {
            parts.push(format!("{} to remove", p.to_remove.len()));
        }
        if p.skipped > 0 {
            parts.push(format!("{} left out (format)", p.skipped));
        }
        if p.playlist_files > 0 {
            parts.push(format!("{} playlist copies", p.playlist_files));
        }
        ui.label(parts.join(" · "));
        if !p.to_remove.is_empty() {
            ui.collapsing(RichText::new("files that will be removed").color(ink.muted).size(13.0), |ui| {
                for r in p.to_remove.iter().take(200) {
                    ui.label(RichText::new(r.display().to_string()).size(12.5).color(ink.muted));
                }
            });
        }
    }
}

fn layout_options(app: &mut App, ui: &mut Ui) {
    let off_recho = crate::library::non_recho_summary(&app.tracks);
    let mut changed = false;
    let w = &mut app.saved.writes[app.saved.active_write];
    caps(ui, "layout");
    ui.horizontal(|ui| {
        muted(ui, "album folders");
        changed |= ui.radio_value(&mut w.options.album_order, AlbumOrder::Year, "1997 - Album").on_hover_text("chronological").changed();
        changed |= ui.radio_value(&mut w.options.album_order, AlbumOrder::Title, "Album").on_hover_text("alphabetical").changed();
        changed |= ui.radio_value(&mut w.options.album_order, AlbumOrder::Custom, "01 - Album").on_hover_text("your own order: use ↑ ↓ below").changed();
    });
    ui.horizontal(|ui| {
        muted(ui, "loose tracks go in");
        changed |= ui.add(egui::TextEdit::singleline(&mut w.options.singles_folder).desired_width(120.0)).changed();
        muted(ui, "playlists in");
        changed |= ui.add(egui::TextEdit::singleline(&mut w.playlists_folder).desired_width(120.0)).changed();
    });
    if let Some((n, formats)) = &off_recho {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            muted(ui, format!("{} RECHO can't play ({formats}):", plural(*n, "song")));
        });
        ui.horizontal(|ui| {
            changed |= ui.radio_value(&mut w.options.skip_unplayable, true, "leave them off the card").changed();
            changed |= ui.radio_value(&mut w.options.skip_unplayable, false, "copy them anyway").changed();
        });
        if !w.options.skip_unplayable {
            theme::warning_frame(ui).show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} will be copied but won't play on the Echo Mini: RECHO plays FLAC, MP3 and WAV only. They'll take {} of space and show up in folders as files that do nothing.",
                        plural(*n, "file"),
                        text::bytes(app.tracks.iter().filter(|t| !t.on_recho()).map(|t| t.size).sum())
                    ))
                    .color(ink(ui).accent),
                );
            });
        }
        ui.add_space(4.0);
    }
    changed |= ui.checkbox(&mut w.options.write_tags, "write corrected tags into the copies").changed();
    changed |= ui.checkbox(&mut w.prune, "remove files this write put on the card before that are no longer selected").changed();
    changed |= ui.checkbox(&mut w.options.clean_mac_files, "delete macOS ._ and .DS_Store files on the card").changed();
    if changed {
        app.write_preview = None;
    }
}

fn playlists(app: &mut App, ui: &mut Ui) {
    caps(ui, "playlists on this card");
    if app.saved.playlists.is_empty() {
        muted(ui, "none yet; make some in the playlists tab.");
        return;
    }
    let ids: Vec<PathBuf> = app.tracks.iter().map(Track::id).collect();
    let by_path = ids.iter().map(PathBuf::as_path).zip(&app.tracks).collect();
    let mut changed = false;
    let w = &mut app.saved.writes[app.saved.active_write];
    ui.horizontal_wrapped(|ui| {
        for p in &app.saved.playlists {
            let mut on = w.playlists.contains(&p.name);
            let label = format!("{} ({}, {})", p.name, p.tracks.len(), text::bytes(p.bytes(&by_path)));
            if ui.checkbox(&mut on, label).changed() {
                changed = true;
                if on {
                    w.playlists.insert(p.name.clone());
                } else {
                    w.playlists.remove(&p.name);
                }
            }
        }
    });
    if changed {
        app.write_preview = None;
    }
}

enum Edit {
    Artist(String, bool),
    Album(String, bool),
    All(bool),
    Expand(String),
    Rename(bool, String, String),
    StartRename(bool, String, String),
    Move(String, Vec<String>, usize, isize),
}

fn selection(app: &mut App, ui: &mut Ui, sizes: &Sizes) {
    let ink = ink(ui);
    let mut edits = Vec::new();
    ui.horizontal(|ui| {
        caps(ui, "what goes on the card");
        ui.add(egui::TextEdit::singleline(&mut app.card_filter).hint_text("filter").desired_width(180.0));
        if link(ui, "all").clicked() {
            edits.push(Edit::All(true));
        }
        if link(ui, "none").clicked() {
            edits.push(Edit::All(false));
        }
    });
    muted(ui, "Names on the right are the folder names on the card; rename changes them only for this write.");
    ui.add_space(4.0);
    let w = app.write();
    let needle = app.card_filter.to_lowercase();
    let custom_order = w.options.album_order == AlbumOrder::Custom;
    for artist in &app.index.artists {
        let ck = &app.index.clusters[artist.cluster].key;
        if !needle.is_empty()
            && !artist.name.to_lowercase().contains(&needle)
            && !artist.albums.iter().any(|a| a.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&needle)))
        {
            continue;
        }
        let included = !w.custom.excluded_artists.contains(ck);
        let ids: Vec<String> = artist.albums.iter().map(|a| a.id(ck)).collect();
        let partly = included && ids.iter().any(|id| w.custom.excluded_albums.contains(id));
        let card_name = w.custom.artist_names.get(ck).cloned().unwrap_or_else(|| artist.name.clone());
        let open = app.card_expanded.contains(ck);
        ui.horizontal(|ui| {
            let mut on = included;
            if ui.add(egui::Checkbox::without_text(&mut on).indeterminate(partly)).changed() {
                edits.push(Edit::Artist(ck.clone(), on));
            }
            let name = RichText::new(format!("{} {}", if open { "▾" } else { "▸" }, artist.name)).size(16.0).color(if included { ink.text } else { ink.muted });
            // Leave room for the size and the "rename" link on the right.
            let r = widgets::fitted(ui, name, ui.available_width() - 180.0, Sense::click()).on_hover_text(&artist.name);
            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                edits.push(Edit::Expand(ck.clone()));
            }
            muted(ui, text::bytes(sizes.artist.get(ck).copied().unwrap_or(0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Clear of the scrollbar.
                ui.add_space(10.0);
                rename_widget(ui, &app.card_renaming, true, ck, &card_name, &mut edits);
            });
        });
        if !open {
            continue;
        }
        let ordered = w.custom.ordered_albums(ck, &artist.albums);
        let order_ids: Vec<String> = ordered.iter().map(|a| a.id(ck)).collect();
        let mut position = 0;
        for (n, al) in ordered.iter().enumerate() {
            let id = &order_ids[n];
            let title = w.custom.album_names.get(id).map(String::as_str);
            let folder = album_folder(al, title, position, &w.options);
            if al.title.is_some() {
                position += 1;
            }
            let on_card = included && !w.custom.excluded_albums.contains(id);
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                let mut on = !w.custom.excluded_albums.contains(id);
                if ui.add_enabled(included, egui::Checkbox::without_text(&mut on)).changed() {
                    edits.push(Edit::Album(id.clone(), on));
                }
                let shown = al.title.as_deref().unwrap_or("Loose tracks");
                let title_text = RichText::new(shown).italics().color(if on_card { ink.text } else { ink.muted });
                widgets::fitted(ui, title_text, ui.available_width() * 0.45, Sense::hover()).on_hover_text(shown);
                muted(ui, format!("{} · {}", plural(al.tracks.len(), "track"), text::bytes(sizes.album.get(id).copied().unwrap_or(0))));
                let off = al.tracks.iter().filter(|&&i| !app.tracks[i].on_recho()).count();
                if off > 0 {
                    let text = if w.options.skip_unplayable { format!("{off} left out (format)") } else { format!("{off} won't play") };
                    ui.label(RichText::new(text).size(12.5).color(ink.accent)).on_hover_text("RECHO plays FLAC, MP3 and WAV only; change this under layout below");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(10.0);
                    if custom_order && al.title.is_some() {
                        if link(ui, "↓").clicked() {
                            edits.push(Edit::Move(ck.clone(), order_ids.clone(), n, 1));
                        }
                        if link(ui, "↑").clicked() {
                            edits.push(Edit::Move(ck.clone(), order_ids.clone(), n, -1));
                        }
                    }
                    if al.title.is_some() {
                        rename_widget(ui, &app.card_renaming, false, id, title.unwrap_or(al.title.as_deref().unwrap_or_default()), &mut edits);
                    }
                    let artist_dir = crate::library::names::sanitize_component(&card_name, 80);
                    let path = format!("{artist_dir} / {folder}");
                    // Whatever space is left between the album and the links, and no more.
                    ui.add(egui::Label::new(RichText::new(text::shorten(&path, 50)).size(12.5).color(ink.muted)).truncate()).on_hover_text(path);
                });
            });
        }
    }

    if edits.is_empty() {
        return;
    }
    let artist_keys: Vec<String> = app.index.artists.iter().map(|a| app.index.clusters[a.cluster].key.clone()).collect();
    let mut touched_layout = false;
    for e in edits {
        let w = &mut app.saved.writes[app.saved.active_write];
        match e {
            Edit::Expand(k) => {
                if !app.card_expanded.remove(&k) {
                    app.card_expanded.insert(k);
                }
            }
            Edit::StartRename(is_artist, key, current) => app.card_renaming = Some((is_artist, key, current)),
            Edit::Rename(is_artist, key, name) => {
                let map = if is_artist { &mut w.custom.artist_names } else { &mut w.custom.album_names };
                if name.trim().is_empty() {
                    map.remove(&key);
                } else {
                    map.insert(key, name.trim().to_string());
                }
                app.card_renaming = None;
                touched_layout = true;
            }
            Edit::Artist(k, on) => {
                if on {
                    w.custom.excluded_artists.remove(&k);
                } else {
                    w.custom.excluded_artists.insert(k);
                }
                touched_layout = true;
            }
            Edit::Album(id, on) => {
                if on {
                    w.custom.excluded_albums.remove(&id);
                } else {
                    w.custom.excluded_albums.insert(id);
                }
                touched_layout = true;
            }
            Edit::All(on) => {
                w.custom.excluded_albums.clear();
                w.custom.excluded_artists = if on { Default::default() } else { artist_keys.iter().cloned().collect() };
                touched_layout = true;
            }
            Edit::Move(ck, mut ids, n, d) => {
                let m = n as isize + d;
                if m >= 0 && (m as usize) < ids.len() {
                    ids.swap(n, m as usize);
                    w.custom.album_order.insert(ck, ids);
                    touched_layout = true;
                }
            }
        }
    }
    if touched_layout {
        app.write_preview = None;
    }
}

/// "rename" link, or the inline editor when this row is being renamed.
fn rename_widget(ui: &mut Ui, renaming: &Option<(bool, String, String)>, is_artist: bool, key: &str, current: &str, edits: &mut Vec<Edit>) {
    match renaming {
        Some((a, k, buf)) if *a == is_artist && k == key => {
            let mut text = buf.clone();
            if link(ui, "save").clicked() {
                edits.push(Edit::Rename(is_artist, key.to_string(), text.clone()));
            }
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(200.0));
            if r.changed() {
                edits.push(Edit::StartRename(is_artist, key.to_string(), text.clone()));
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                edits.push(Edit::Rename(is_artist, key.to_string(), text));
            }
        }
        _ => {
            if link(ui, "rename").on_hover_text("folder name on this card only; empty resets").clicked() {
                edits.push(Edit::StartRename(is_artist, key.to_string(), current.to_string()));
            }
        }
    }
}
