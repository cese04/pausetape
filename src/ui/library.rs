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
use crate::card::Player;
use crate::library::Track;
use super::layout::{ICON, S1, S2, S4, S6, row, row_wrapped};

/// Longest folder path shown before it's shortened with "…" in the middle.
const FOLDER_CHARS: usize = 60;

/// Where a song or album stands with the card, as of the last write against what's chosen now.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mark {
    /// On the card and staying.
    On,
    /// Chosen; the next Sync copies it (for an album, some of it may be there already).
    Adding,
    /// On the card, copied there by hand: taking the card over moves it into place.
    ByHand,
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
    pub(super) fn paint(self, p: &egui::Painter, at: egui::Pos2, r: f32, ink: &theme::Ink) {
        match self {
            Mark::On => {
                p.circle_filled(at, r, ink.muted);
            }
            Mark::Adding => {
                p.circle_stroke(at, r, Stroke::new(1.2, ink.text));
            }
            Mark::ByHand => {
                p.circle_stroke(at, r, Stroke::new(1.2, ink.text));
                p.circle_filled(at, r * 0.45, ink.text);
            }
            Mark::Removing => {
                p.circle_stroke(at, r, Stroke::new(1.2, ink.warn));
                p.hline(at.x - r - 1.5..=at.x + r + 1.5, at.y, Stroke::new(1.2, ink.warn));
            }
        }
    }
}

/// Where an album's track numbers end: clear of the card mark at the left even for "2-10", and
/// the same for every song of the album, so their titles line up.
fn number_column(ui: &Ui, numbers: &[String]) -> f32 {
    let widest = numbers.iter().map(|n| ui.painter().layout_no_wrap(n.clone(), FontId::proportional(14.5), egui::Color32::PLACEHOLDER).size().x).fold(0.0, f32::max);
    (14.0 + widest).max(30.0)
}

/// A track row painted directly so columns line up like a table without any grid lines.
/// `number`: the track number, and where the album's numbers end (`number_column`).
/// `repeat`: an extra copy of a song the album already has. `selected`: chosen for editing tags.
#[allow(clippy::too_many_arguments)]
fn track_row(ui: &mut Ui, t: &Track, device: Player, (number, number_end): (&str, f32), show_artist: bool, playing: bool, can_add: bool, mark: Option<Mark>, repeat: bool, selected: bool) -> (egui::Response, bool) {
    let ink = ink(ui);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, t.display_title()));
    widgets::focus_ring(ui, &resp, rect, 4.0);
    widgets::row_background(ui, &resp, rect, selected);
    let p = ui.painter_at(rect);
    let y = rect.center().y;
    let font = FontId::proportional(14.5);
    let num_color = if playing { ink.accent } else { ink.muted };
    p.text(egui::pos2(rect.left() + number_end, y), Align2::RIGHT_CENTER, if playing { "▸" } else { number }, font.clone(), num_color);
    if let Some(m) = mark {
        m.paint(&p, egui::pos2(rect.left() + 6.0, y), 2.4, &ink);
    }

    let mut title = t.display_title();
    if show_artist && let Some(a) = &t.artist {
        title = format!("{title}  ·  {a}");
    }
    let right_cols = if device.plays(t) && !repeat { 130.0 } else { 250.0 };
    let title_x = number_end + 14.0;
    let galley = truncated(ui, title, font.clone(), if t.plays_here() { ink.text } else { ink.muted }, width - title_x - right_cols);
    p.galley(egui::pos2(rect.left() + title_x, y - galley.size().y / 2.0), galley, ink.text);

    let mut x = rect.right() - 28.0;
    p.text(egui::pos2(x, y), Align2::RIGHT_CENTER, text::duration(t.duration), font.clone(), ink.muted);
    x -= 56.0;
    if repeat {
        p.text(egui::pos2(x, y), Align2::RIGHT_CENTER, "stored twice", FontId::proportional(12.0), ink.warn);
    } else if !device.plays(t) {
        // A small outlined badge: the format, and what it means for the player.
        let label = text::format_badge(t, device);
        let g = ui.painter().layout_no_wrap(label, FontId::proportional(11.5), ink.warn);
        let badge = egui::Rect::from_min_size(egui::pos2(x - g.size().x - 8.0, y - g.size().y / 2.0 - 2.0), g.size() + vec2(8.0, 4.0));
        p.rect_stroke(badge, 3.0, Stroke::new(0.8, ink.warn), egui::StrokeKind::Inside);
        p.galley(badge.min + vec2(4.0, 2.0), g, ink.warn);
    } else if t.dirty {
        p.text(egui::pos2(x, y), Align2::RIGHT_CENTER, "edited", FontId::proportional(12.0), ink.muted);
    }
    let mut added = false;
    if can_add {
        let plus = egui::Rect::from_center_size(egui::pos2(rect.right() - 8.0, y), vec2(16.0, ROW_H));
        let r = ui.interact(plus, resp.id.with("add"), Sense::click()).on_hover_text("add to playlist");
        widgets::name(ui, &r, egui::Role::Button, "add to playlist");
        widgets::focus_ring(ui, &r, plus, 2.0);
        if resp.hovered() || r.hovered() || r.has_focus() {
            p.text(plus.center(), Align2::CENTER_CENTER, "+", font, if r.hovered() { ink.text } else { ink.muted });
        }
        added = r.clicked();
    }
    let resp = match text::format_note(t, device) {
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

/// The A–Z rail's width.
const RAIL_W: f32 = S4;

pub fn show(app: &mut App, ui: &mut Ui) {
    // The details of what's selected, at the right edge, where there's room for them; the
    // library in the rest.
    app.details_beside = super::details::fits(ui.available_width()) && !app.tracks.is_empty();
    if app.details_beside {
        let mut actions = Vec::new();
        let frame = egui::Frame::new().inner_margin(egui::Margin { left: S2 as i8, ..Default::default() });
        egui::Panel::right("details").exact_size(super::details::WIDTH + S2).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| super::details::show(app, ui, &mut actions));
        apply(app, actions);
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| page(app, ui));
}

fn page(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let device = app.player();
    // What's shown, in words, and what can be done with all of it. (The folder, the views and
    // what's worth a look are in the sidebar; the search box is in the top bar.)
    row_wrapped(ui, |ui| {
        let title = if app.duplicates_only {
            "Duplicates".to_string()
        } else if app.look_alikes_only {
            "Artists that look alike".to_string()
        } else {
            let filter = app.filter.trim().to_string();
            match filter.as_str() {
                "" if app.saved.album_table => "Albums".to_string(),
                "" => "Artists".to_string(),
                "cover:no" => "Without a cover".to_string(),
                "damaged:yes" => "Damaged files".to_string(),
                "edited:yes" => "Corrected, not in the files".to_string(),
                "genre:none" => "No genre".to_string(),
                f if f.starts_with("genre:") && !f.contains(' ') => {
                    let k = crate::library::genres::key(&f["genre:".len()..]);
                    let name = app.genre_index().genres.iter().find(|g| g.key == k).map(|g| g.name.clone());
                    name.unwrap_or_else(|| f["genre:".len()..].to_string())
                }
                f => format!("Searching “{f}”"),
            }
        };
        let in_genre = app.filter.trim().starts_with("genre:") && !app.filter.trim().contains(' ');
        if in_genre && widgets::icon_link(ui, Icon::Back, "All genres").clicked() {
            app.tab = crate::app::Tab::Genres;
        }
        ui.label(RichText::new(title).size(20.0).color(ink.text));
        if app.source_missing {
            ui.label(RichText::new("can't reach the music folder: is its drive connected? It's read again once it's back").color(ink.warn));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let dirty = app.dirty_in_files();
            if dirty > 0 && widgets::icon_link(ui, Icon::Check, format!("Write {dirty} corrected tags to source files")).clicked() {
                app.save_tags_to_source();
            }
            if !app.saved.album_table && widgets::icon_link(ui, Icon::Collapse, "Collapse all").clicked() {
                app.expanded.clear();
            }
        });
    });
    // Songs the card's player can't play are said in their rows (a badge) and dealt with in the
    // card tab, where converting them is chosen.
    ui.add_space(S2);
    if app.tracks.is_empty() {
        // An unreachable folder is said in red above.
        if !app.source_missing {
            muted(ui, "Choose the folder that holds your music: the menu at the top of the sidebar, or Settings › Music folder. It can be as messy as it likes; nothing in it changes unless you ask.");
        }
        return;
    }

    if app.write().dest.is_some() {
        crate::ui::card::strip(app, ui);
        ui.add_space(S1);
    }
    let mut actions = Vec::new();
    // One song selected is only a highlight (a click selects); from two, a bar says what to do
    // with them. Esc clears either.
    if app.selected.len() > 1 {
        super::tag_editor::selection_bar(app, ui, &mut actions);
    } else if !app.selected.is_empty() && app.tag_edit.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        actions.push(Action::ClearSelection);
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
    if app.look_alikes_only && app.index.suggestions.is_empty() {
        app.look_alikes_only = false;
        app.notice("no look-alike artists left · showing the whole library");
    }
    if app.look_alikes_only {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.look_alikes_only = false;
        }
        theme::panel_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            row_wrapped(ui, |ui| {
                if widgets::button_with(ui, Icon::Back, "Back to the whole library").on_hover_text("or press Esc").clicked() {
                    app.look_alikes_only = false;
                }
                ui.add_space(S1);
                ui.label(RichText::new("Showing only artists whose names look like another's. Say under each whether they're the same artist.").color(ink.text));
            });
        });
        ui.add_space(S1);
    }
    if app.duplicates_only {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.duplicates_only = false;
        }
        theme::panel_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            row_wrapped(ui, |ui| {
                if widgets::button_with(ui, Icon::Back, "Back to the whole library").on_hover_text("or press Esc").clicked() {
                    app.duplicates_only = false;
                }
                ui.add_space(S1);
                ui.label(RichText::new("Showing only albums with songs stored twice, or a copy that another copy covers in the same or higher quality.").color(ink.text));
            });
            let spare = app.index.superseded();
            if !spare.is_empty() {
                ui.add_space(S1);
                let bytes: u64 = spare.iter().flat_map(|&(a, b)| &app.index.artists[a].albums[b].tracks).map(|&i| app.tracks[i].size).sum();
                let hint = "Each copy goes only while the copy that covers it is still there: its folder, or only its songs if the folder holds other music. You can put them back from the Trash.";
                if app.busy.is_none() && widgets::icon_link(ui, Icon::Trash, format!("Move all {} covered copies to the Trash ({})", spare.len(), text::bytes(bytes))).on_hover_text(hint).clicked() {
                    actions.push(Action::TrashCopies(spare));
                }
            }
            let repeats = app.index.repeats();
            if !repeats.is_empty() {
                ui.add_space(S1);
                let bytes: u64 = repeats.iter().map(|&(c, _)| app.tracks[c].size).sum();
                let hint = "Each extra copy is checked against the copy that stays first, and goes only if they're still the same file. You can put them back from the Trash.";
                if app.busy.is_none() && widgets::icon_link(ui, Icon::Trash, format!("Move all {} extra copies to the Trash ({})", repeats.len(), text::bytes(bytes))).on_hover_text(hint).clicked() {
                    actions.push(Action::TrashRepeats(repeats));
                }
            }
        });
        ui.add_space(S1);
    }
    let query = app.query();
    if query.wants(&crate::library::search::Filter::Cover(false)) {
        covers_panel(app, ui, &query, &mut actions);
    }
    if app.saved.album_table {
        super::albums::show(app, ui, &query, &mut actions);
        apply(app, actions);
        return;
    }
    let current = app.current();
    let can_add = app.active_playlist.is_some();
    let by_genre = query.filters.iter().any(|(f, wanted)| *wanted && matches!(f, crate::library::search::Filter::Genre(k) if !k.is_empty()));
    let mods = ui.input(|i| i.modifiers);
    let present: std::collections::BTreeSet<char> = app.index.artists.iter().map(|a| initial(&a.name)).collect();
    let jump = app.jump_to.take();
    let source_root = app.saved.source.clone().unwrap_or_default();
    // Songs in the order they're shown, for the arrow keys; the one they last moved to.
    let mut shown: Vec<usize> = Vec::new();
    let scroll_to = app.scroll_to_song.take();
    // The A–Z rail is a panel of its own at the right edge; the list fills the rest.
    let mut rail_click = None;
    let rail = egui::Frame::new().inner_margin(egui::Margin { left: S1 as i8, ..Default::default() });
    egui::Panel::right("az rail").exact_size(RAIL_W).resizable(false).show_separator_line(false).frame(rail).show(ui, |ui| {
        let height = ui.available_height();
        rail_click = az_rail(ui, &present, height);
    });
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let mut last_initial = None;
            for (ai, artist) in app.index.artists.iter().enumerate() {
                let akey = crate::library::names::artist_key(&artist.name);
                let ck = &app.index.clusters[artist.cluster].key;
                let matching: Vec<usize> = if app.duplicates_only {
                    (0..artist.albums.len()).filter(|&i| duplicated.contains(&artist.albums[i].base_id(ck))).collect()
                } else if query.is_empty() {
                    (0..artist.albums.len()).collect()
                } else {
                    (0..artist.albums.len()).filter(|&i| query.album_matches(&app.album_facts(ai, i))).collect()
                };
                if matching.is_empty() || app.look_alikes_only && !app.index.suggestions.iter().any(|s| s.a == artist.cluster || s.b == artist.cluster) {
                    continue;
                }
                let open = app.expanded.contains(&akey) || app.duplicates_only || (!query.is_empty() && matching.len() < 6);
                let n_tracks: usize = artist.albums.iter().map(|a| a.tracks.len()).sum();
                // Letter headings, which the A–Z rail scrolls to.
                let letter = initial(&artist.name);
                if last_initial != Some(letter) {
                    last_initial = Some(letter);
                    ui.add_space(S2);
                    let r = ui.label(RichText::new(letter.to_string()).size(13.0).color(ink.muted).strong());
                    ui.painter().hline(r.rect.right() + 8.0..=ui.max_rect().right(), r.rect.center().y, Stroke::new(0.5, ink.faint));
                    if jump == Some(letter) {
                        r.scroll_to_me(Some(Align::TOP));
                    }
                }
                ui.add_space(S1);
                let custom = &app.write().custom;
                let whole = custom.whole_artist(ck);
                let r = ui
                    .horizontal(|ui| {
                        let meta = format!("{} · {}", plural(artist.albums.len(), "album"), plural(n_tracks, "track"));
                        let card_link = if whole { "Take all off the card" } else { "Send all to the card" };
                        let name_w = ui.available_width() - widgets::text_width(ui, &meta, 15.5) - widgets::text_width(ui, card_link, 14.0) - 70.0;
                        let name = RichText::new(&artist.name).size(20.0).color(ink.text);
                        let r = widgets::fitted(ui, name, name_w, Sense::click()).on_hover_text(&artist.name);
                        widgets::focus_ring(ui, &r, r.rect, 2.0);
                        r.context_menu(|ui| super::menu::music(app, ui, super::menu::Of::Artist(ai), &mut actions, |_, _| {}));
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
                    actions.push(Action::ToggleArtist(akey.clone(), ai));
                }
                super::artist_panel::look_alikes(app, ui, artist.cluster, &mut actions);
                if !open {
                    continue;
                }
                // Opened by hand, not by a search or a filter: who they are, where to buy more.
                if app.expanded.contains(&akey) {
                    super::artist_panel::show(app, ui, artist.cluster, &mut actions);
                }
                for &bi in &matching {
                    let al = &artist.albums[bi];
                    let id = al.id(&app.index.clusters[artist.cluster].key);
                    ui.add_space(S1);
                    // The album's commands, in the order they read: (icon, words, hint, what it does).
                    let no_cover = app.lacks_cover(al);
                    let chosen = app.write().custom.includes(ck, al, &artist.albums);
                    let mut commands: Vec<(Icon, String, String, Option<Action>)> = vec![(Icon::Play, "Play".into(), String::new(), Some(Action::Play(al.tracks.clone(), 0)))];
                    if no_cover && app.busy.is_none() {
                        commands.push((Icon::Image, "Find cover".into(), covers_hint(device, "No cover in the songs. Looks in the album's folder, then on the Cover Art Archive."), Some(Action::FindCovers(vec![(ai, bi)]))));
                    }
                    commands.push((Icon::Search, "Look up".into(), "suggest tags from MusicBrainz".into(), Some(Action::LookupAlbum(ai, bi))));
                    commands.push((Icon::Rename, "Edit tags".into(), "Album, artists, year, numbers and titles. To edit any songs together, ⌘-click them.".into(), Some(Action::EditTags(al.tracks.clone()))));
                    // A copy with all these songs in higher quality: offer it instead.
                    if chosen
                        && let Some(s) = al.edition.as_ref().and_then(|e| e.superseded.as_ref()).filter(|s| s.higher)
                        && let Some(better) = artist.albums.iter().position(|o| o.base_id(ck) == al.base_id(ck) && o.edition.as_ref().is_some_and(|e| e.rank == s.rank))
                    {
                        let hint = format!("{} has all these songs in higher quality. It replaces this copy on the card at the next Sync.", s.label);
                        commands.push((Icon::Copy, format!("Use the {} copy", s.format), hint, Some(Action::CardAlbum(ai, better, true))));
                    }
                    commands.push(if chosen {
                        (Icon::Remove, "Take off the card".into(), "Takes it off the card at the next Sync.".into(), Some(Action::CardAlbum(ai, bi, false)))
                    } else {
                        (Icon::Add, "Send to the card".into(), "Copies it to the card at the next Sync. Of several editions, this one goes.".into(), Some(Action::CardAlbum(ai, bi, true)))
                    });
                    let commands_w: f32 = commands.iter().map(|(_, words, ..)| S1 + ICON + S1 + widgets::text_width(ui, words, 13.5) + S1 + ui.spacing().item_spacing.x).sum();
                    let draw = |ui: &mut Ui, commands: &mut Vec<(Icon, String, String, Option<Action>)>, actions: &mut Vec<Action>, backwards: bool| {
                        let order: Vec<usize> = if backwards { (0..commands.len()).rev().collect() } else { (0..commands.len()).collect() };
                        for i in order {
                            let (icon, words, hint, action) = &mut commands[i];
                            let r = widgets::icon_link(ui, *icon, words.as_str());
                            let r = if hint.is_empty() { r } else { r.on_hover_text(hint.as_str()) };
                            if r.clicked()
                                && let Some(a) = action.take()
                            {
                                actions.push(a);
                            }
                        }
                    };
                    let mut below = false;
                    row(ui, |ui| {
                        ui.add_sized([44.0, 20.0], egui::Label::new(RichText::new(al.year.map(|y| y.to_string()).unwrap_or_default()).color(ink.muted)));
                        let title = al.title.as_deref().unwrap_or("Loose tracks");
                        let mut meta = format!("{} · {}", plural(al.tracks.len(), "track"), album_minutes(&app.tracks, &al.tracks));
                        if let Some(e) = &al.edition {
                            meta = format!("edition {} of {} · {} · {meta}", e.rank + 1, e.count, e.format);
                        }
                        let (mark, status) = album_status(app, ck, artist, al);
                        let off = al.tracks.iter().filter(|&&i| !device.plays(&app.tracks[i])).count();
                        let off_text = match off {
                            0 => String::new(),
                            n if n == al.tracks.len() => format!("won't play on {}", device.short()),
                            n => format!("{n} won't play on {}", device.short()),
                        };
                        // The title gets whatever the counts, warning and the album's commands leave;
                        // where the commands would crowd it (a narrow window), they go on a line below.
                        let words = widgets::text_width(ui, &meta, 15.5) + widgets::text_width(ui, &off_text, 12.5) + widgets::text_width(ui, &status, 13.0) + 3.0 * S2 + 10.0;
                        below = ui.available_width() < words + commands_w + 120.0;
                        let reserve = words + if below { 0.0 } else { commands_w };
                        let shown = RichText::new(title).italics().size(17.0);
                        let r = widgets::fitted(ui, shown, ui.available_width() - reserve, Sense::click()).on_hover_text(format!("{title}\nRight-click for what you can do with it."));
                        widgets::focus_ring(ui, &r, r.rect, 2.0);
                        r.context_menu(|ui| super::menu::music(app, ui, super::menu::Of::Album(ai, bi), &mut actions, |_, _| {}));
                        let r = ui.label(RichText::new(meta).color(ink.muted));
                        if let Some(e) = &al.edition {
                            r.on_hover_text(format!("{}\nThe card gets one edition; choose which in the card tab.", e.label));
                        }
                        if off > 0 {
                            ui.label(RichText::new(off_text).size(12.5).color(ink.warn));
                        }
                        if let Some(m) = mark {
                            let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                            m.paint(ui.painter(), rect.center(), 3.5, &ink);
                            ui.label(RichText::new(&status).size(13.0).color(if m == Mark::Removing { ink.warn } else { ink.muted }));
                        }
                        // In a genre only by its artist's genres (no song of it names one): say so.
                        if by_genre && app.album_genres(ai, bi).from_artist {
                            ui.label(RichText::new("by the artist's genre").size(12.5).italics().color(ink.muted))
                                .on_hover_text("No song of this album names a genre; its artist's, from MusicBrainz, put it here. Look it up to have its own suggested.");
                        }
                        if !below {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| draw(ui, &mut commands, &mut actions, true));
                        }
                    });
                    if below {
                        row_wrapped(ui, |ui| {
                            ui.add_space(S6);
                            draw(ui, &mut commands, &mut actions, false);
                        });
                    }
                    album_folders(ui, &app.tracks, &al.tracks, &source_root);
                    if let Some(s) = al.edition.as_ref().and_then(|e| e.superseded.as_ref()) {
                        superseded_note(ui, &app.tracks, &al.tracks, s, app.busy.is_none(), || actions.push(Action::TrashCopies(vec![(ai, bi)])));
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
                    let album_name = al.title.as_deref().unwrap_or("");
                    let every_song = query.names_match(&artist.name, album_name);
                    let numbers: Vec<String> = al
                        .tracks
                        .iter()
                        .map(|&ti| {
                            let t = &app.tracks[ti];
                            match (t.track, al.multi_disc) {
                                (Some(n), true) => format!("{}-{n}", t.disc.unwrap_or(1)),
                                (Some(n), false) => n.to_string(),
                                (None, _) => String::new(),
                            }
                        })
                        .collect();
                    let number_end = number_column(ui, &numbers);
                    for (pos, &ti) in al.tracks.iter().enumerate() {
                        let t = &app.tracks[ti];
                        if !every_song && !query.song_matches(&artist.name, album_name, &t.display_title()) {
                            continue;
                        }
                        let mark = Mark::of(app.card_state.on_card.contains(&ti), app.card_state.planned.contains(&ti));
                        let selected = app.selected.contains(&ti);
                        let (r, add) = track_row(ui, t, device, (&numbers[pos], number_end), al.various || al.title.is_none(), current == Some(ti), can_add, mark, al.is_repeat(ti), selected);
                        shown.push(ti);
                        if scroll_to == Some(ti) {
                            r.scroll_to_me(None);
                        }
                        // Dragged onto a playlist: the selection it's in, or the song.
                        super::drag::songs_from(&r, || if selected && app.selected.len() > 1 { app.selected.iter().copied().collect() } else { vec![ti] });
                        // As in the album table and any file list: a click selects, a double click
                        // plays (from this song to the album's end).
                        if r.double_clicked() {
                            actions.push(Action::Play(al.tracks.clone(), pos));
                        } else if r.clicked() && mods.command {
                            actions.push(Action::Select(ti));
                        } else if r.clicked() && mods.shift {
                            // A run from the last song clicked, if it's in this album.
                            let from = app.select_anchor.and_then(|a| al.tracks.iter().position(|&x| x == a)).unwrap_or(pos);
                            actions.push(Action::SelectRun(al.tracks[from.min(pos)..=from.max(pos)].to_vec()));
                        } else if r.clicked() {
                            actions.push(Action::OnlySong(ti));
                        }
                        r.context_menu(|ui| {
                            // On a song in the selection, the menu is for the whole selection.
                            let songs = if selected && app.selected.len() > 1 { app.selected.iter().copied().collect() } else { vec![ti] };
                            super::menu::music(app, ui, super::menu::Of::Songs(songs), &mut actions, |ui, actions| {
                                ui.separator();
                                if widgets::icon_link(ui, if selected { Icon::Remove } else { Icon::Add }, if selected { "Take out of the selection (⌘-click)" } else { "Add to the selection (⌘-click)" }).clicked() {
                                    actions.push(Action::Select(ti));
                                    ui.close();
                                }
                            });
                        });
                        if add {
                            actions.push(Action::Add(ti));
                        }
                    }
                }
            }
        });
    });
    if let Some(c) = rail_click {
        app.jump_to = Some(c);
    }
    song_keys(app, ui, &shown, &mut actions);
    apply(app, actions);
}

/// The keyboard in the library by artist: ↑/↓ move the selection through the songs shown (with
/// Shift, extend it), Enter plays, ⌘A selects them all, ⌘I edits tags, ⌘D sends their albums
/// to the card or takes them off.
fn song_keys(app: &App, ui: &Ui, shown: &[usize], actions: &mut Vec<Action>) {
    if shown.is_empty() || super::keys::taken(app, ui) {
        return;
    }
    let navigation = super::keys::navigation_free(app, ui);
    let at = app.select_anchor.and_then(|a| shown.iter().position(|&t| t == a));
    for (key, step) in [(egui::Key::ArrowDown, 1isize), (egui::Key::ArrowUp, -1)] {
        if let Some(extend) = super::keys::nav_key(ui, key).filter(|_| navigation) {
            let next = match at {
                Some(i) => (i as isize + step).clamp(0, shown.len() as isize - 1) as usize,
                None if step > 0 => 0,
                None => shown.len() - 1,
            };
            actions.push(Action::SongCursor(shown[next], extend));
        }
    }
    if navigation && super::keys::nav_key(ui, egui::Key::Enter).is_some() && !app.selected.is_empty() {
        let picked: Vec<usize> = shown.iter().copied().filter(|t| app.selected.contains(t)).collect();
        if let [one] = picked[..] {
            // One song plays on through its album, as a double click does.
            if let Some(&(ai, bi)) = app.albums_of(&[one].into()).first() {
                let al = &app.index.artists[ai].albums[bi];
                actions.push(Action::Play(al.tracks.clone(), al.tracks.iter().position(|&t| t == one).unwrap_or(0)));
            }
        } else if !picked.is_empty() {
            actions.push(Action::Play(picked, 0));
        }
    }
    if super::keys::pressed(ui, &super::keys::SELECT_ALL) {
        actions.push(Action::SelectRun(shown.to_vec()));
    }
    if !app.selected.is_empty() && super::keys::pressed(ui, &super::keys::EDIT_TAGS) {
        actions.push(Action::EditTags(app.selected.iter().copied().collect()));
    }
    if !app.selected.is_empty() && super::keys::pressed(ui, &super::keys::CARD) {
        actions.push(Action::ToggleCard(app.albums_of(&app.selected)));
    }
}

/// An album's mark and words for where it stands with the card ("on card", "added at the next
/// sync"…), or no mark and "" when it isn't chosen and isn't there.
pub(super) fn album_status(app: &App, ck: &str, artist: &crate::library::grouping::ArtistNode, al: &crate::library::grouping::AlbumNode) -> (Option<Mark>, String) {
    let cs = &app.card_state;
    let planned = al.tracks.iter().filter(|i| cs.planned.contains(i)).count();
    let there = al.tracks.iter().filter(|i| cs.on_card.contains(i)).count();
    let staying = al.tracks.iter().filter(|i| cs.planned.contains(i) && cs.on_card.contains(i)).count();
    let hand = al.tracks.iter().filter(|i| cs.by_hand.contains(i)).count();
    if hand > 0 && there == 0 {
        let what = if hand == al.tracks.len() { "on the card".to_string() } else { format!("{hand} of {} on the card", al.tracks.len()) };
        let then = if planned > 0 { "; Take over moves it into place" } else { "" };
        return (Some(Mark::ByHand), format!("{what}, copied by hand{then}"));
    }
    match (planned, there) {
        (0, 0) => (None, String::new()),
        (0, _) => {
            // Another copy of the album taking its place?
            let swapped = artist.albums.iter().any(|o| o.base_id(ck) == al.base_id(ck) && o.tracks.iter().any(|i| cs.planned.contains(i)));
            (Some(Mark::Removing), if swapped { "swapped for the other copy at the next Sync" } else { "taken off at the next Sync" }.into())
        }
        (p, _) if staying == p => (Some(Mark::On), "on the card".into()),
        (_, 0) => (Some(Mark::Adding), "added at the next Sync".into()),
        (p, _) => (Some(Mark::Adding), format!("{staying} of {p} on the card, the rest at the next Sync")),
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
        if has {
            widgets::name(ui, &resp, egui::Role::Link, &format!("artists under {c}"));
        }
        if (resp.hovered() || resp.has_focus()) && has {
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
fn superseded_note(ui: &mut Ui, tracks: &[Track], idxs: &[usize], s: &crate::library::grouping::Superseded, can_trash: bool, trash: impl FnOnce()) {
    let ink = ink(ui);
    let bytes: u64 = idxs.iter().map(|&i| tracks[i].size).sum();
    let how = if s.higher { format!("in higher quality ({})", s.format) } else { format!("in the same quality ({})", s.format) };
    row_wrapped(ui, |ui| {
        ui.add_space(S6);
        ui.label(RichText::new(format!("Edition {} has all these songs {how}; this copy could go ({}).", s.rank + 1, text::bytes(bytes))).size(12.5).italics().color(ink.text))
            .on_hover_text(format!("Edition {}: {}.\nUntil this copy goes, the card gets only one edition anyway.", s.rank + 1, s.label));
        let hint = "Moves this copy's folder to the Trash (or only its songs, if the folder holds other music), as long as the copy that covers it is still there. You can put it back from the Trash.";
        if can_trash && widgets::icon_link(ui, Icon::Trash, "Move it to the Trash").on_hover_text(hint).clicked() {
            trash();
        }
    });
}

/// What a missing cover means on the card's player, then `then`.
fn covers_hint(device: Player, then: &str) -> String {
    let shows = if device.shows_covers() {
        format!("{} shows covers embedded in the songs.", device.name())
    } else {
        format!("{} shows no covers, but this app's player and most others show the ones embedded in the songs.", device.short())
    };
    format!("{shows} {then}")
}

/// Songs the album holds twice: how many, what they take, and a way to bin the extra copies.
fn repeats_note(ui: &mut Ui, app: &App, al: &crate::library::grouping::AlbumNode, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let bytes: u64 = al.repeats.iter().map(|&(c, _)| app.tracks[c].size).sum();
    let n = al.repeats.len();
    row_wrapped(ui, |ui| {
        ui.add_space(S6);
        let songs = if n == 1 { "1 song is".to_string() } else { format!("{n} songs are") };
        ui.label(RichText::new(format!("{songs} stored twice here ({} extra); the card gets each once.", text::bytes(bytes))).size(12.5).italics().color(ink.warn));
        let hint = "Moves the extra copies to the Trash, each only if it's still the same file as the copy that stays (the one with the shorter name). You can put them back from the Trash.";
        if app.busy.is_none() && widgets::icon_link(ui, Icon::Trash, "Move the extra copies to the Trash").on_hover_text(hint).clicked() {
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
    row(ui, |ui| {
        ui.add_space(S6);
        let text = match dirs.len() {
            1 => text::shorten(&rel(first), FOLDER_CHARS),
            n => format!("{} and {} more folders", text::shorten(&rel(first), FOLDER_CHARS - 20), n - 1),
        };
        // "open folder" goes first so a long path can never push it out of view.
        if widgets::icon_link(ui, Icon::Folder, "Open folder").clicked() {
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
pub(super) fn lookup_block(ui: &mut Ui, tracks: &[Track], id: &str, l: &mut Lookup, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    ui.indent(id, |ui| {
        match l {
            Lookup::Pending(_) => {
                muted(ui, "asking MusicBrainz…");
            }
            Lookup::Failed(e) => {
                row_wrapped(ui, |ui| {
                    muted(ui, format!("MusicBrainz: {e}"));
                    if widgets::icon_link(ui, Icon::Close, "Dismiss").clicked() {
                        actions.push(Action::Dismiss(id.to_string()));
                    }
                });
            }
            Lookup::Ready { label, changes } => {
                let n = changes.iter().filter(|c| c.accept).count();
                let card = theme::panel_frame(ui).inner_margin(egui::Margin::symmetric(S2 as i8, S1 as i8));
                card.show(ui, |ui| {
                    row_wrapped(ui, |ui| {
                        widgets::icon(ui, Icon::Search, ink.accent);
                        caps(ui, "musicbrainz suggests");
                        ui.label(RichText::new(label.as_str()).color(ink.text));
                        if !changes.is_empty() {
                            muted(ui, format!("· {n} of {} chosen", changes.len()));
                        }
                    });
                    if changes.is_empty() {
                        row(ui, |ui| {
                            widgets::icon(ui, Icon::Check, ink.muted);
                            muted(ui, "Your tags already agree.");
                            if widgets::icon_link(ui, Icon::Close, "Dismiss").clicked() {
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
                    ui.add_space(S1 / 2.0);
                    for key in order {
                        let idxs = &groups[&key];
                        let first = &changes[idxs[0]];
                        let on = idxs.iter().all(|&i| changes[i].accept);
                        let scope = if idxs.len() > 1 { format!("{} songs", idxs.len()) } else { tracks[first.track].display_title() };
                        if suggestion_row(ui, first, &scope, on).clicked() {
                            for &i in idxs {
                                changes[i].accept = !on;
                            }
                        }
                    }
                    ui.add_space(S1);
                    row_wrapped(ui, |ui| {
                        ui.add_enabled_ui(n > 0, |ui| {
                            let text = if n == 0 { "Apply".to_string() } else { format!("Apply {}", plural(n, "change")) };
                            if widgets::primary_with(ui, Icon::Check, text).on_hover_text("Corrects these tags in the library; the files change when you write the corrected tags.").clicked() {
                                actions.push(Action::ApplyLookup(id.to_string()));
                            }
                        });
                        if widgets::button_with(ui, Icon::Close, "Dismiss").on_hover_text("Keeps your tags as they are.").clicked() {
                            actions.push(Action::Dismiss(id.to_string()));
                        }
                        ui.add_space(S1);
                        if n < changes.len() && link(ui, "Choose all").clicked() {
                            changes.iter_mut().for_each(|c| c.accept = true);
                        }
                        if n > 0 && link(ui, "None").clicked() {
                            changes.iter_mut().for_each(|c| c.accept = false);
                        }
                    });
                });
            }
        }
    });
    ui.add_space(S1);
}

/// One suggested change, a click turning it on or off. What the tag will be once applied is in
/// ink, what goes is struck through: chosen, the old value goes and the new one comes, ticked;
/// left out, the old value stays and the new one is struck.
fn suggestion_row(ui: &mut Ui, c: &crate::musicbrainz::FieldChange, scope: &str, on: bool) -> egui::Response {
    let ink = ink(ui);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, format!("{}: {} to {}", c.field.label(), c.old, c.new)));
    widgets::focus_ring(ui, &resp, rect, 4.0);
    let p = ui.painter_at(rect);
    if resp.hovered() || resp.has_focus() {
        p.rect_filled(rect, 4.0, ink.hover);
    }
    let y = rect.center().y;
    // The tick: filled in the accent when chosen, an empty ring when not.
    let at = egui::pos2(rect.left() + S1 + ICON / 2.0, y);
    if on {
        p.circle_filled(at, ICON / 2.0, ink.accent);
        icons::paint(&p, egui::Rect::from_center_size(at, vec2(ICON, ICON)).shrink(4.0), Icon::Check, ink.bg);
    } else {
        p.circle_stroke(at, ICON / 2.0 - 0.6, Stroke::new(1.2, if resp.hovered() { ink.text } else { ink.muted }));
    }
    let small = FontId::proportional(12.5);
    let font = FontId::proportional(14.5);
    let mut x = rect.left() + S1 + ICON + S2;
    // What changes takes up to a third of the row; the values, the rest.
    let label_w = (width * 0.3).clamp(72.0, 160.0);
    let what = truncated(ui, format!("{} · {scope}", c.field.label()), small.clone(), ink.muted, label_w);
    p.galley(egui::pos2(x, y - what.size().y / 2.0), what, ink.muted);
    x += label_w + S2;
    let room = (rect.right() - x - S1).max(0.0);
    let struck = |text: &str, color, max_w| {
        let mut job = egui::text::LayoutJob::default();
        job.append(text, 0.0, egui::TextFormat { font_id: font.clone(), color, strikethrough: Stroke::new(1.0, color), ..Default::default() });
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
        ui.painter().layout_job(job)
    };
    let plain = |text: &str, color, max_w| truncated(ui, text.to_string(), font.clone(), color, max_w);
    let arrow_w = S1 + ICON + S1;
    let (old, new) = if c.old.is_empty() {
        let empty = plain("empty", ink.faint, room / 2.0);
        (empty, if on { plain(&c.new, ink.text, room / 2.0) } else { struck(&c.new, ink.muted, room / 2.0) })
    } else if on {
        (struck(&c.old, ink.muted, (room - arrow_w) / 2.0), plain(&c.new, ink.text, (room - arrow_w) / 2.0))
    } else {
        (plain(&c.old, ink.text, (room - arrow_w) / 2.0), struck(&c.new, ink.muted, (room - arrow_w) / 2.0))
    };
    let old_w = old.size().x;
    p.galley(egui::pos2(x, y - old.size().y / 2.0), old, ink.text);
    x += old_w + S1;
    icons::paint(&p, egui::Rect::from_min_size(egui::pos2(x, y - ICON / 2.0), vec2(ICON, ICON)).shrink(2.0), Icon::Forward, if on { ink.accent } else { ink.faint });
    x += ICON + S1;
    p.galley(egui::pos2(x, y - new.size().y / 2.0), new, ink.text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(if on { "Chosen: click to keep your tag" } else { "Left out: click to take this one" })
}

/// Search terms worth a click, toggled in the search box.
const QUICK_FILTERS: &[(&str, &str)] = &[
    ("card:on", "on the card"),
    ("card:off", "not on the card, nor chosen"),
    ("card:sync", "changing at the next Sync"),
    ("card:better", "on the card while a better copy exists"),
    ("cover:no", "without a cover"),
    ("quality:lossy", "with lossy songs (MP3, AAC…)"),
    ("quality:hires", "hi-res"),
    ("edited:yes", "with corrected tags not yet in the files"),
];

/// A menu of common search terms; ticking one adds it to the search box, unticking removes it.
pub(super) fn filters_menu(app: &mut App, ui: &mut Ui) {
    let active = QUICK_FILTERS.iter().filter(|(term, _)| app.filter.split_whitespace().any(|w| w.eq_ignore_ascii_case(term))).count();
    let label = if active > 0 { format!("Filters ({active})") } else { "Filters".to_string() };
    ui.menu_button(RichText::new(label).color(ink(ui).muted), |ui| {
        for (term, what) in QUICK_FILTERS {
            let mut on = app.filter.split_whitespace().any(|w| w.eq_ignore_ascii_case(term));
            if ui.checkbox(&mut on, format!("{what}  ")).on_hover_text(*term).changed() {
                let mut words: Vec<&str> = app.filter.split_whitespace().filter(|w| !w.eq_ignore_ascii_case(term)).collect();
                if on {
                    words.push(term);
                }
                app.filter = words.join(" ");
            }
        }
        ui.separator();
        widgets::help(ui, "Or type them, with other terms: hover over the search box for the list.");
    });
}

/// Above albums without a cover: find covers for them, and what the last search couldn't find.
fn covers_panel(app: &mut App, ui: &mut Ui, query: &crate::library::search::Query, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let albums: Vec<(usize, usize)> = app.albums_without_cover().into_iter().filter(|&(a, b)| query.album_matches(&app.album_facts(a, b))).collect();
    theme::panel_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        row_wrapped(ui, |ui| {
            let what = if albums.is_empty() { "No albums without a cover here.".to_string() } else { format!("{} without a cover shown.", plural(albums.len(), "album")) };
            ui.label(RichText::new(what).color(ink.text));
            if !albums.is_empty() && app.busy.is_none() {
                let hint = "For each album: an image in its folder (cover.jpg, folder.jpg…) if there is one, else the front cover of the matching MusicBrainz release, from the Cover Art Archive (about two seconds an album). Nothing changes in your files until you write corrected tags.";
                if widgets::button_with(ui, Icon::Image, format!("Find covers for {}", if albums.len() == 1 { "it".to_string() } else { format!("these {}", albums.len()) })).on_hover_text(hint).clicked() {
                    actions.push(Action::FindCovers(albums.clone()));
                }
            }
            if widgets::icon_link(ui, Icon::Close, "Show everything").clicked() {
                app.filter.clear();
            }
        });
        if !app.cover_misses.is_empty() {
            ui.add_space(S1);
            let n = app.cover_misses.len();
            ui.collapsing(RichText::new(format!("not found last time: {}", plural(n, "album"))).size(13.0).color(ink.muted), |ui| {
                for (album, why) in app.cover_misses.iter().take(300) {
                    ui.add(egui::Label::new(RichText::new(format!("{album}: {why}")).size(12.5).color(ink.muted)).truncate());
                }
            });
            widgets::help(ui, "Fixing an album's name (look up, or edit tags) often helps. Or put a cover.jpg in its folder.");
        }
    });
    ui.add_space(S1);
}
