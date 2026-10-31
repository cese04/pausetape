//! The library tab: artists, albums and tracks, with letter headings, an A–Z rail, album
//! folders, format badges and MusicBrainz suggestions.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use egui::{Align, Align2, FontId, Layout, RichText, ScrollArea, Sense, Stroke, Ui, vec2};

use super::actions::{Action, apply};
use super::icons::{self, Icon};
use super::text::{self, plural};
use super::theme::{self, ink};
use super::widgets::{self, caps, link, muted, truncated};
use super::ROW_H;
use crate::app::{App, Lookup};
use crate::library::Track;

/// Longest folder path shown before it's shortened with "…" in the middle.
const FOLDER_CHARS: usize = 60;

/// Where a song or album stands with the card, as of the last write against what's chosen now.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    /// On the card and staying.
    On,
    /// Chosen; the next Sync copies it (for an album, some of it may be there already).
    Adding,
    /// On the card; the next Sync takes it off.
    Removing,
}

impl Mark {
    fn of(on_card: bool, planned: bool) -> Option<Mark> {
        match (on_card, planned) {
            (true, true) => Some(Mark::On),
            (false, true) => Some(Mark::Adding),
            (true, false) => Some(Mark::Removing),
            (false, false) => None,
        }
    }

    /// A dot like a library card's stamp: filled when there, a ring while waiting for Sync.
    fn paint(self, p: &egui::Painter, at: egui::Pos2, r: f32, ink: &theme::Ink) {
        match self {
            Mark::On => {
                p.circle_filled(at, r, ink.muted);
            }
            Mark::Adding => {
                p.circle_stroke(at, r, Stroke::new(1.2, ink.text));
            }
            Mark::Removing => {
                p.circle_stroke(at, r, Stroke::new(1.2, ink.accent));
                p.hline(at.x - r - 1.5..=at.x + r + 1.5, at.y, Stroke::new(1.2, ink.accent));
            }
        }
    }
}

/// A track row painted directly so columns line up like a table without any grid lines.
/// `repeat`: an extra copy of a song the album already has. `selected`: chosen for editing tags.
#[allow(clippy::too_many_arguments)]
fn track_row(ui: &mut Ui, t: &Track, number: &str, show_artist: bool, playing: bool, can_add: bool, mark: Option<Mark>, repeat: bool, selected: bool) -> (egui::Response, bool) {
    let ink = ink(ui);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect, 4.0, ink.chip);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, ink.hover);
    }
    let y = rect.center().y;
    let font = FontId::proportional(14.5);
    let num_color = if playing { ink.accent } else { ink.muted };
    p.text(egui::pos2(rect.left() + 30.0, y), Align2::RIGHT_CENTER, if playing { "▸" } else { number }, font.clone(), num_color);
    if let Some(m) = mark {
        m.paint(&p, egui::pos2(rect.left() + 6.0, y), 2.4, &ink);
    }

    let mut title = t.display_title();
    if show_artist && let Some(a) = &t.artist {
        title = format!("{title}  ·  {a}");
    }
    let right_cols = if t.on_recho() && !repeat { 130.0 } else { 250.0 };
    let galley = truncated(ui, title, font.clone(), if t.plays_here() { ink.text } else { ink.muted }, width - 44.0 - right_cols);
    p.galley(egui::pos2(rect.left() + 44.0, y - galley.size().y / 2.0), galley, ink.text);

    let mut x = rect.right() - 28.0;
    p.text(egui::pos2(x, y), Align2::RIGHT_CENTER, text::duration(t.duration), font.clone(), ink.muted);
    x -= 56.0;
    if repeat {
        p.text(egui::pos2(x, y), Align2::RIGHT_CENTER, "stored twice", FontId::proportional(12.0), ink.accent);
    } else if !t.on_recho() {
        // A small outlined badge: the format, and what it means for the player.
        let label = text::format_badge(t);
        let g = ui.painter().layout_no_wrap(label, FontId::proportional(11.5), ink.accent);
        let badge = egui::Rect::from_min_size(egui::pos2(x - g.size().x - 8.0, y - g.size().y / 2.0 - 2.0), g.size() + vec2(8.0, 4.0));
        p.rect_stroke(badge, 3.0, Stroke::new(0.8, ink.accent), egui::StrokeKind::Inside);
        p.galley(badge.min + vec2(4.0, 2.0), g, ink.accent);
    } else if t.dirty {
        p.text(egui::pos2(x, y), Align2::RIGHT_CENTER, "edited", FontId::proportional(12.0), ink.muted);
    }
    let mut added = false;
    if can_add {
        let plus = egui::Rect::from_center_size(egui::pos2(rect.right() - 8.0, y), vec2(16.0, ROW_H));
        let r = ui.interact(plus, resp.id.with("add"), Sense::click()).on_hover_text("add to playlist");
        if resp.hovered() || r.hovered() {
            p.text(plus.center(), Align2::CENTER_CENTER, "+", font, if r.hovered() { ink.text } else { ink.muted });
        }
        added = r.clicked();
    }
    let resp = match text::format_note(t) {
        Some(note) => resp.on_hover_text(note),
        None if repeat => resp.on_hover_text(format!("Another file in this album is the same song (same number, size and length). The card gets it once.\n{}", t.path.display())),
        None => resp,
    };
    (resp, added)
}

fn album_minutes(tracks: &[Track], idxs: &[usize]) -> String {
    let total: Duration = idxs.iter().map(|&i| tracks[i].duration).sum();
    format!("{} min", total.as_secs().div_ceil(60))
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    // The music folder and what's in it.
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), rect, Icon::Folder, ink.text);
        let name = app.saved.source.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "not chosen".into());
        ui.label(RichText::new(name).size(18.0).color(ink.text)).on_hover_text(app.saved.source.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        let albums: usize = app.index.artists.iter().map(|a| a.albums.len()).sum();
        let mut facts = format!("{} · {} · {}", plural(app.tracks.len(), "song"), plural(app.index.artists.len(), "artist"), plural(albums, "album"));
        let dirty = app.dirty_count();
        if dirty > 0 {
            facts.push_str(&format!(" · {} with corrected tags", plural(dirty, "song")));
        }
        muted(ui, facts);
        if widgets::icon_link(ui, Icon::Folder, "change folder").clicked() {
            app.picker = Some(crate::ui::folders::Picker::Source(crate::ui::navigator::Navigator::new(app.saved.source.as_deref())));
        }
        if app.busy.is_none() && widgets::icon_link(ui, Icon::Rescan, "rescan").on_hover_text("pick up files added or changed since").clicked() {
            app.scan();
        }
    });
    ui.add_space(4.0);
    // Searching, and what's worth a look.
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), rect, Icon::Search, ink.muted);
        ui.add(egui::TextEdit::singleline(&mut app.filter).hint_text("filter by artist, album or title").desired_width(280.0));
        if widgets::icon_link(ui, Icon::Collapse, "collapse all").clicked() {
            app.expanded.clear();
        }
        let dirty = app.dirty_in_files();
        if dirty > 0 && widgets::icon_link(ui, Icon::Check, format!("write {dirty} corrected tags to source files")).clicked() {
            app.save_tags_to_source();
        }
        ui.add_space(8.0);
        let spare = app.index.superseded();
        if !spare.is_empty() {
            let bytes: u64 = spare.iter().flat_map(|&(a, b)| &app.index.artists[a].albums[b].tracks).map(|&i| app.tracks[i].size).sum();
            let copies = if spare.len() == 1 { "album copy" } else { "album copies" };
            let hint = "Copies whose songs are all in another copy of the album, in the same or higher quality. Click to show only them.";
            if widgets::chip(ui, Icon::Copy, format!("{} {copies} covered by another ({})", spare.len(), text::bytes(bytes)), ink.text).on_hover_text(hint).clicked() {
                app.duplicates_only = true;
            }
        }
        let repeats = app.index.repeats();
        if !repeats.is_empty() {
            let bytes: u64 = repeats.iter().map(|&(c, _)| app.tracks[c].size).sum();
            let hint = "Songs kept twice in the same album, like the copies a backup tool leaves. Click to show only their albums.";
            if widgets::chip(ui, Icon::Copy, format!("{} stored twice ({})", plural(repeats.len(), "song"), text::bytes(bytes)), ink.text).on_hover_text(hint).clicked() {
                app.duplicates_only = true;
            }
        }
        if !app.index.suggestions.is_empty()
            && widgets::chip(ui, Icon::Artists, format!("{} possible duplicate artists", app.index.suggestions.len()), ink.text).on_hover_text("Artists whose names look alike. Decide in the artists tab.").clicked()
        {
            app.tab = crate::app::Tab::Artists;
        }
    });
    if let Some((n, formats)) = crate::library::non_recho_summary(&app.tracks) {
        ui.add_space(6.0);
        let collapsed = app.saved.format_note_collapsed;
        theme::warning_frame(ui).show(ui, |ui| {
            ui.set_max_width(ui.available_width() - RAIL_W - 40.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} in formats the Echo Mini can't play: {formats}", plural(n, "song"))).color(ink.accent));
                if link(ui, if collapsed { "why?" } else { "hide" }).clicked() {
                    app.saved.format_note_collapsed = !collapsed;
                }
            });
            if !collapsed {
                muted(
                    ui,
                    if crate::audio::ffmpeg().is_some() {
                        "RECHO plays FLAC, MP3 and WAV only. Here, AAC, ALAC, AIFF and Ogg directly; Dolby (E-AC-3, AC-3), Opus, APE, WavPack and WMA by converting with ffmpeg first, in stereo; Dolby AC-4 has no decoder yet. On the card they're converted (lossless to FLAC, the rest to MP3 320) unless you choose otherwise in the card tab."
                    } else {
                        "RECHO plays FLAC, MP3 and WAV only. AAC, ALAC, AIFF and Ogg still play here; Dolby (E-AC-3, AC-3), Opus, APE, WavPack and WMA need ffmpeg (brew install ffmpeg). On the card they're converted (lossless to FLAC, the rest to MP3 320) unless you choose otherwise in the card tab."
                    },
                );
            }
        });
    }
    ui.add_space(10.0);
    if app.tracks.is_empty() {
        muted(ui, "Choose the folder that holds your music in the folders tab. It can be as messy as it likes; nothing in it changes unless you ask.");
        return;
    }

    if app.write().dest.is_some() {
        crate::ui::card::strip(app, ui);
        ui.add_space(6.0);
    }
    let mut actions = Vec::new();
    if !app.selected.is_empty() {
        super::tag_editor::selection_bar(app, ui, &mut actions);
    }
    // Albums (by base id) where one copy covers another, or with songs stored twice, for the
    // duplicates view.
    let duplicated: std::collections::HashSet<String> = app
        .index
        .superseded()
        .into_iter()
        .chain(app.index.artists.iter().enumerate().flat_map(|(a, artist)| artist.albums.iter().enumerate().filter(|(_, al)| !al.repeats.is_empty()).map(move |(b, _)| (a, b))))
        .map(|(a, b)| app.index.artists[a].albums[b].base_id(&app.index.clusters[app.index.artists[a].cluster].key))
        .collect();
    // Once the copies are dealt with (and the rescan is done), there's nothing left to show.
    if app.duplicates_only && duplicated.is_empty() && app.busy.is_none() {
        app.duplicates_only = false;
        app.notice("no duplicates left · showing the whole library");
    }
    if app.duplicates_only {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.duplicates_only = false;
        }
        theme::panel_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if widgets::button_with(ui, Icon::Back, "back to the whole library").on_hover_text("or press Esc").clicked() {
                    app.duplicates_only = false;
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Showing only albums with songs stored twice, or a copy that another copy covers in the same or higher quality.").color(ink.text));
            });
            let repeats = app.index.repeats();
            if !repeats.is_empty() {
                ui.add_space(6.0);
                let bytes: u64 = repeats.iter().map(|&(c, _)| app.tracks[c].size).sum();
                let hint = "Each extra copy is checked against the copy that stays first, and goes only if they're still the same file. You can put them back from the Trash.";
                if app.busy.is_none() && widgets::icon_link(ui, Icon::Trash, format!("move all {} extra copies to the Trash ({})", repeats.len(), text::bytes(bytes))).on_hover_text(hint).clicked() {
                    actions.push(Action::TrashRepeats(repeats));
                }
            }
        });
        ui.add_space(8.0);
    }
    let needle = app.filter.to_lowercase();
    let current = app.current();
    let can_add = app.active_playlist.is_some();
    let mods = ui.input(|i| i.modifiers);
    let present: std::collections::BTreeSet<char> = app.index.artists.iter().map(|a| initial(&a.name)).collect();
    let jump = app.jump_to.take();
    let source_root = app.saved.source.clone().unwrap_or_default();
    // The list fills the window up to the rail, which sits at the right edge.
    const RAIL_W: f32 = 28.0;
    let height = ui.available_height();
    let list_w = (ui.available_width() - RAIL_W - 12.0).max(240.0);
    let mut rail_click = None;
    ui.horizontal_top(|ui| {
        // Cells of a horizontal row inherit its left-to-right layout unless told otherwise.
        ui.allocate_ui_with_layout(vec2(list_w, height), Layout::top_down(Align::Min), |ui| {
            ui.set_width(list_w);
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut last_initial = None;
                for (ai, artist) in app.index.artists.iter().enumerate() {
                    let akey = crate::library::names::artist_key(&artist.name);
                    let hit = |s: &str| s.to_lowercase().contains(&needle);
                    let ck = &app.index.clusters[artist.cluster].key;
                    let matching: Vec<usize> = if app.duplicates_only {
                        (0..artist.albums.len()).filter(|&i| duplicated.contains(&artist.albums[i].base_id(ck))).collect()
                    } else if needle.is_empty() {
                        (0..artist.albums.len()).collect()
                    } else if hit(&artist.name) {
                        (0..artist.albums.len()).collect()
                    } else {
                        (0..artist.albums.len())
                            .filter(|&i| {
                                let al = &artist.albums[i];
                                al.title.as_deref().is_some_and(hit) || al.tracks.iter().any(|&t| hit(&app.tracks[t].display_title()))
                            })
                            .collect()
                    };
                    if matching.is_empty() {
                        continue;
                    }
                    let open = app.expanded.contains(&akey) || app.duplicates_only || (!needle.is_empty() && matching.len() < 6);
                    let n_tracks: usize = artist.albums.iter().map(|a| a.tracks.len()).sum();
                    // Letter headings, which the A–Z rail scrolls to.
                    let letter = initial(&artist.name);
                    if last_initial != Some(letter) {
                        last_initial = Some(letter);
                        ui.add_space(10.0);
                        let r = ui.label(RichText::new(letter.to_string()).size(13.0).color(ink.muted).strong());
                        ui.painter().hline(r.rect.right() + 8.0..=ui.max_rect().right(), r.rect.center().y, Stroke::new(0.5, ink.faint));
                        if jump == Some(letter) {
                            r.scroll_to_me(Some(Align::TOP));
                        }
                    }
                    ui.add_space(6.0);
                    let custom = &app.write().custom;
                    let whole = custom.whole_artist(ck);
                    let r = ui
                        .horizontal(|ui| {
                            let meta = format!("{} · {}", plural(artist.albums.len(), "album"), plural(n_tracks, "track"));
                            let card_link = if whole { "take all off the card" } else { "send all to the card" };
                            let name_w = ui.available_width() - widgets::text_width(ui, &meta, 15.5) - widgets::text_width(ui, card_link, 14.0) - 70.0;
                            let name = RichText::new(&artist.name).size(20.0).color(ink.text);
                            let r = widgets::fitted(ui, name, name_w, Sense::click()).on_hover_text(&artist.name);
                            muted(ui, meta);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let hint = if whole {
                                    "The whole artist is chosen for the card, albums added later too. Takes every album off at the next Sync."
                                } else {
                                    "Chooses the whole artist for the card, albums added later too (one edition of each). Copied at the next Sync."
                                };
                                if widgets::icon_link(ui, if whole { Icon::Remove } else { Icon::Add }, card_link).on_hover_text(hint).clicked() {
                                    actions.push(Action::CardArtist(ai, !whole));
                                }
                            });
                            r
                        })
                        .inner;
                    if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        actions.push(Action::ToggleArtist(akey.clone()));
                    }
                    if !open {
                        continue;
                    }
                    for &bi in &matching {
                        let al = &artist.albums[bi];
                        let id = al.id(&app.index.clusters[artist.cluster].key);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add_sized([44.0, 20.0], egui::Label::new(RichText::new(al.year.map(|y| y.to_string()).unwrap_or_default()).color(ink.muted)));
                            let title = al.title.as_deref().unwrap_or("Loose tracks");
                            let mut meta = format!("{} · {}", plural(al.tracks.len(), "track"), album_minutes(&app.tracks, &al.tracks));
                            if let Some(e) = &al.edition {
                                meta = format!("edition {} of {} · {} · {meta}", e.rank + 1, e.count, e.format);
                            }
                            let (mark, status) = album_status(app, ck, artist, al);
                            let off = al.tracks.iter().filter(|&&i| !app.tracks[i].on_recho()).count();
                            let off_text = match off {
                                0 => String::new(),
                                n if n == al.tracks.len() => "won't play on RECHO".to_string(),
                                n => format!("{n} won't play on RECHO"),
                            };
                            // The title gets whatever the counts, warning and the album's commands leave.
                            let reserve = widgets::text_width(ui, &meta, 15.5) + widgets::text_width(ui, &off_text, 12.5) + widgets::text_width(ui, &status, 13.0) + 510.0;
                            let shown = RichText::new(title).italics().size(17.0);
                            widgets::fitted(ui, shown, ui.available_width() - reserve, Sense::hover()).on_hover_text(title);
                            let r = ui.label(RichText::new(meta).color(ink.muted));
                            if let Some(e) = &al.edition {
                                r.on_hover_text(format!("{}\nThe card gets one edition; choose which in the card tab.", e.label));
                            }
                            if off > 0 {
                                ui.label(RichText::new(off_text).size(12.5).color(ink.accent));
                            }
                            if let Some(m) = mark {
                                let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                                m.paint(ui.painter(), rect.center(), 3.5, &ink);
                                ui.label(RichText::new(&status).size(13.0).color(if m == Mark::Removing { ink.accent } else { ink.muted }));
                            }
                            let chosen = app.write().custom.includes(ck, al, &artist.albums);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let (text, hint) = if chosen {
                                    ("take off card", "Takes it off the card at the next Sync.")
                                } else {
                                    ("send to card", "Copies it to the card at the next Sync. Of several editions, this one goes.")
                                };
                                if widgets::icon_link(ui, if chosen { Icon::Remove } else { Icon::Add }, text).on_hover_text(hint).clicked() {
                                    actions.push(Action::CardAlbum(ai, bi, !chosen));
                                }
                                // A copy with all these songs in higher quality: offer it instead.
                                if chosen
                                    && let Some(s) = al.edition.as_ref().and_then(|e| e.superseded.as_ref()).filter(|s| s.higher)
                                    && let Some(better) = artist.albums.iter().position(|o| o.base_id(ck) == al.base_id(ck) && o.edition.as_ref().is_some_and(|e| e.rank == s.rank))
                                    && widgets::icon_link(ui, Icon::Copy, format!("use the {} copy", s.format))
                                        .on_hover_text(format!("{} has all these songs in higher quality. It replaces this copy on the card at the next Sync.", s.label))
                                        .clicked()
                                {
                                    actions.push(Action::CardAlbum(ai, better, true));
                                }
                                if widgets::icon_link(ui, Icon::Rename, "edit tags")
                                    .on_hover_text("Album, artists, year, numbers and titles. To edit any songs together, ⌘-click them.")
                                    .clicked()
                                {
                                    actions.push(Action::EditTags(al.tracks.clone()));
                                }
                                if widgets::icon_link(ui, Icon::Search, "look up").on_hover_text("suggest tags from MusicBrainz").clicked() {
                                    actions.push(Action::LookupAlbum(ai, bi));
                                }
                                if widgets::icon_link(ui, Icon::Play, "play").clicked() {
                                    actions.push(Action::Play(al.tracks.clone(), 0));
                                }
                            });
                        });
                        album_folders(ui, &app.tracks, &al.tracks, &source_root);
                        if let Some(s) = al.edition.as_ref().and_then(|e| e.superseded.as_ref()) {
                            superseded_note(ui, &app.tracks, &al.tracks, s);
                        }
                        if !al.repeats.is_empty() {
                            repeats_note(ui, app, al, &mut actions);
                        }
                        if let Some(l) = app.lookups.get_mut(&id) {
                            lookup_block(ui, &app.tracks, &id, l, &mut actions);
                        }
                        // The duplicates view compares copies; their song lists would bury that.
                        if app.duplicates_only {
                            continue;
                        }
                        for (pos, &ti) in al.tracks.iter().enumerate() {
                            let t = &app.tracks[ti];
                            if !needle.is_empty() && !hit(&artist.name) && !al.title.as_deref().is_some_and(hit) && !hit(&t.display_title()) {
                                continue;
                            }
                            let num = match (t.track, al.multi_disc) {
                                (Some(n), true) => format!("{}-{n}", t.disc.unwrap_or(1)),
                                (Some(n), false) => n.to_string(),
                                (None, _) => String::new(),
                            };
                            let mark = Mark::of(app.card_state.on_card.contains(&ti), app.card_state.planned.contains(&ti));
                            let selected = app.selected.contains(&ti);
                            let (r, add) = track_row(ui, t, &num, al.various || al.title.is_none(), current == Some(ti), can_add, mark, al.is_repeat(ti), selected);
                            if r.clicked() && mods.command {
                                actions.push(Action::Select(ti));
                            } else if r.clicked() && mods.shift {
                                // A run from the last song clicked, if it's in this album.
                                let from = app.select_anchor.and_then(|a| al.tracks.iter().position(|&x| x == a)).unwrap_or(pos);
                                actions.push(Action::SelectRun(al.tracks[from.min(pos)..=from.max(pos)].to_vec()));
                            } else if r.double_clicked() || r.clicked() {
                                actions.push(Action::Play(al.tracks.clone(), pos));
                            }
                            r.context_menu(|ui| {
                                let several = selected && app.selected.len() > 1;
                                if several && widgets::icon_link(ui, Icon::Rename, format!("edit tags of the {} selected songs", app.selected.len())).clicked() {
                                    actions.push(Action::EditTags(app.selected.iter().copied().collect()));
                                    ui.close();
                                }
                                if widgets::icon_link(ui, Icon::Rename, "edit tags of this song").clicked() {
                                    actions.push(Action::EditTags(vec![ti]));
                                    ui.close();
                                }
                                if widgets::icon_link(ui, Icon::Rename, "edit tags of the album").clicked() {
                                    actions.push(Action::EditTags(al.tracks.clone()));
                                    ui.close();
                                }
                                if widgets::icon_link(ui, if selected { Icon::Remove } else { Icon::Add }, if selected { "unselect (⌘-click)" } else { "select (⌘-click)" }).clicked() {
                                    actions.push(Action::Select(ti));
                                    ui.close();
                                }
                            });
                            if add {
                                actions.push(Action::Add(ti));
                            }
                        }
                    }
                }
            });
        });
        ui.add_space(12.0);
        ui.allocate_ui_with_layout(vec2(RAIL_W, height), Layout::top_down(Align::Min), |ui| {
            rail_click = az_rail(ui, &present, height);
        });
    });
    if let Some(c) = rail_click {
        app.jump_to = Some(c);
    }
    apply(app, actions);
}

/// An album's mark and words for where it stands with the card ("on card", "added at the next
/// sync"…), or no mark and "" when it isn't chosen and isn't there.
fn album_status(app: &App, ck: &str, artist: &crate::library::grouping::ArtistNode, al: &crate::library::grouping::AlbumNode) -> (Option<Mark>, String) {
    let cs = &app.card_state;
    let planned = al.tracks.iter().filter(|i| cs.planned.contains(i)).count();
    let there = al.tracks.iter().filter(|i| cs.on_card.contains(i)).count();
    let staying = al.tracks.iter().filter(|i| cs.planned.contains(i) && cs.on_card.contains(i)).count();
    match (planned, there) {
        (0, 0) => (None, String::new()),
        (0, _) => {
            // Another copy of the album taking its place?
            let swapped = artist.albums.iter().any(|o| o.base_id(ck) == al.base_id(ck) && o.tracks.iter().any(|i| cs.planned.contains(i)));
            (Some(Mark::Removing), if swapped { "swapped for the other copy at the next sync" } else { "taken off at the next sync" }.into())
        }
        (p, _) if staying == p => (Some(Mark::On), "on card".into()),
        (_, 0) => (Some(Mark::Adding), "added at the next sync".into()),
        (p, _) => (Some(Mark::Adding), format!("{staying} of {p} on card, the rest at the next sync")),
    }
}

/// Index letter for an artist: "The Beatles" → B, "Beyoncé" → B, "2Pac" → #.
fn initial(name: &str) -> char {
    match crate::library::names::artist_key(name).chars().next() {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase(),
        _ => '#',
    }
}

/// The A–Z rail; letters without artists are faint. Returns the clicked letter.
fn az_rail(ui: &mut Ui, present: &std::collections::BTreeSet<char>, height: f32) -> Option<char> {
    let ink = ink(ui);
    let mut letters: Vec<char> = std::iter::once('#').chain('A'..='Z').collect();
    // Too short for all 27 at a readable size: keep only the letters that lead somewhere.
    if height / (letters.len() as f32) < 13.0 {
        letters.retain(|c| present.contains(c));
    }
    let row = (height / letters.len().max(1) as f32).clamp(9.0, 20.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut clicked = None;
    for c in letters {
        let has = present.contains(&c);
        let sense = if has { Sense::click() } else { Sense::hover() };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row), sense);
        if resp.hovered() && has {
            ui.painter().circle_filled(rect.center(), row * 0.48, ink.faint);
        }
        let color = if has { ink.text } else { ink.faint };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, c, FontId::proportional((row * 0.72).min(13.0)), color);
        if resp.on_hover_cursor(if has { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Default }).clicked() {
            clicked = Some(c);
        }
    }
    clicked
}

/// A copy whose songs another edition has in the same or higher quality: say so, and what
/// removing it would free. Whole copies are left for the user to delete.
fn superseded_note(ui: &mut Ui, tracks: &[Track], idxs: &[usize], s: &crate::library::grouping::Superseded) {
    let ink = ink(ui);
    let bytes: u64 = idxs.iter().map(|&i| tracks[i].size).sum();
    let how = if s.higher { format!("in higher quality ({})", s.format) } else { format!("in the same quality ({})", s.format) };
    ui.horizontal_wrapped(|ui| {
        ui.add_space(52.0);
        ui.label(RichText::new(format!("Edition {} has all these songs {how}; this copy could go ({}).", s.rank + 1, text::bytes(bytes))).size(12.5).italics().color(ink.text))
            .on_hover_text(format!(
                "Edition {}: {}.\nTo remove this copy, open its folder and delete it yourself. Until then, the card gets only one edition anyway.",
                s.rank + 1,
                s.label
            ));
    });
}

/// Songs the album holds twice: how many, what they take, and a way to bin the extra copies.
fn repeats_note(ui: &mut Ui, app: &App, al: &crate::library::grouping::AlbumNode, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let bytes: u64 = al.repeats.iter().map(|&(c, _)| app.tracks[c].size).sum();
    let n = al.repeats.len();
    ui.horizontal_wrapped(|ui| {
        ui.add_space(52.0);
        let songs = if n == 1 { "1 song is".to_string() } else { format!("{n} songs are") };
        ui.label(RichText::new(format!("{songs} stored twice here ({} extra); the card gets each once.", text::bytes(bytes))).size(12.5).italics().color(ink.accent));
        let hint = "Moves the extra copies to the Trash, each only if it's still the same file as the copy that stays (the one with the shorter name). You can put them back from the Trash.";
        if app.busy.is_none() && widgets::icon_link(ui, Icon::Trash, "move the extra copies to the Trash").on_hover_text(hint).clicked() {
            actions.push(Action::TrashRepeats(al.repeats.clone()));
        }
    });
}

/// Where an album's files live on disk, relative to the music folder, with a way to open it.
fn album_folders(ui: &mut Ui, tracks: &[Track], idxs: &[usize], root: &Path) {
    let ink = ink(ui);
    let mut dirs: Vec<&Path> = idxs.iter().filter_map(|&i| tracks[i].path.parent()).collect();
    dirs.sort();
    dirs.dedup();
    let Some(first) = dirs.first().copied() else { return };
    let rel = |d: &Path| {
        let r = d.strip_prefix(root).unwrap_or(d).display().to_string();
        if r.is_empty() { "(top of the music folder)".to_string() } else { r }
    };
    ui.horizontal(|ui| {
        ui.add_space(52.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        let text = match dirs.len() {
            1 => text::shorten(&rel(first), FOLDER_CHARS),
            n => format!("{} and {} more folders", text::shorten(&rel(first), FOLDER_CHARS - 20), n - 1),
        };
        // "open folder" goes first so a long path can never push it out of view.
        if widgets::icon_link(ui, Icon::Folder, "open folder").clicked() {
            crate::platform::reveal(first);
        }
        if idxs.iter().any(|&i| tracks[i].cue.is_some()) {
            ui.label(RichText::new("one file + CUE sheet ·").size(12.5).color(ink.muted))
                .on_hover_text("This album is stored as a single file; its CUE sheet lists the songs. On the card each song becomes its own FLAC file.");
        }
        let r = ui.add(egui::Label::new(RichText::new(text).size(12.5).color(ink.muted)).truncate());
        r.on_hover_text(dirs.iter().map(|d| rel(d)).collect::<Vec<_>>().join("\n"));
    });
}

/// MusicBrainz suggestions under an album. Identical changes across tracks collapse into one line.
fn lookup_block(ui: &mut Ui, tracks: &[Track], id: &str, l: &mut Lookup, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    ui.indent(id, |ui| {
        match l {
            Lookup::Pending => {
                muted(ui, "asking MusicBrainz…");
            }
            Lookup::Failed(e) => {
                ui.horizontal_wrapped(|ui| {
                    muted(ui, format!("MusicBrainz: {e}"));
                    if widgets::icon_link(ui, Icon::Close, "dismiss").clicked() {
                        actions.push(Action::Dismiss(id.to_string()));
                    }
                });
            }
            Lookup::Ready { label, changes } => {
                ui.horizontal_wrapped(|ui| {
                    caps(ui, "musicbrainz");
                    ui.label(RichText::new(label.as_str()).color(ink.text));
                });
                if changes.is_empty() {
                    ui.horizontal(|ui| {
                        muted(ui, "tags already agree.");
                        if widgets::icon_link(ui, Icon::Close, "dismiss").clicked() {
                            actions.push(Action::Dismiss(id.to_string()));
                        }
                    });
                    return;
                }
                let mut groups: BTreeMap<(usize, String, String), Vec<usize>> = BTreeMap::new();
                let mut order = Vec::new();
                for (i, c) in changes.iter().enumerate() {
                    let key = (c.field as usize, c.old.clone(), c.new.clone());
                    if !groups.contains_key(&key) {
                        order.push(key.clone());
                    }
                    groups.entry(key).or_default().push(i);
                }
                for key in order {
                    let idxs = &groups[&key];
                    let first = changes[idxs[0]].clone();
                    let mut on = idxs.iter().all(|&i| changes[i].accept);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut on, "").changed() {
                            for &i in idxs {
                                changes[i].accept = on;
                            }
                        }
                        let scope = if idxs.len() > 1 { format!("{} tracks", idxs.len()) } else { tracks[first.track].display_title() };
                        ui.add_sized([150.0, 18.0], egui::Label::new(RichText::new(format!("{} · {scope}", first.field.label())).size(12.5).color(ink.muted)).truncate());
                        if !first.old.is_empty() {
                            ui.label(RichText::new(&first.old).strikethrough().color(ink.muted));
                            muted(ui, "→");
                        }
                        ui.label(&first.new);
                    });
                }
                ui.horizontal(|ui| {
                    if widgets::icon_link(ui, Icon::Check, "apply selected").clicked() {
                        actions.push(Action::ApplyLookup(id.to_string()));
                    }
                    if widgets::icon_link(ui, Icon::Close, "dismiss").clicked() {
                        actions.push(Action::Dismiss(id.to_string()));
                    }
                });
            }
        }
    });
    ui.add_space(4.0);
}
