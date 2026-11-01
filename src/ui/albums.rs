//! The library as a table of albums: sortable columns, the search box's filters, and a
//! selection of albums to act on together.

use egui::{Align2, FontId, RichText, ScrollArea, Sense, Stroke, Ui, vec2};

use super::ROW_H;
use super::actions::Action;
use super::icons::{self, Icon};
use super::library::{Mark, album_status};
use super::text::{self, plural};
use super::theme::{self, ink};
use super::widgets::{self, muted, truncated};
use crate::app::{AlbumRow, App, SortBy};
use crate::library::search::Query;

/// Fixed columns: (heading, sort, width, right-aligned). Artist and album share what's left.
const FIXED: [(&str, SortBy, f32, bool); 6] = [
    ("year", SortBy::Year, 54.0, false),
    ("format", SortBy::Format, 130.0, false),
    ("songs", SortBy::Songs, 52.0, true),
    ("size", SortBy::Size, 92.0, true),
    ("card", SortBy::Card, 120.0, false),
    ("cover", SortBy::Cover, 56.0, false),
];
const MARK_W: f32 = 16.0;
/// Space after a right-aligned number, so the next column doesn't run into it.
const RIGHT_PAD: f32 = 18.0;

pub fn show(app: &mut App, ui: &mut Ui, query: &Query, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let rows = app.album_rows(query);
    if !app.selected_albums.is_empty() {
        selection_bar(app, ui, &rows, actions);
    } else if !query.is_empty() && !rows.is_empty() {
        ui.horizontal(|ui| {
            muted(ui, format!("{} match", plural(rows.len(), "album")));
            if widgets::icon_link(ui, Icon::Check, "select them all").on_hover_text("to send them to the card, edit their tags or find their covers together").clicked() {
                actions.push(Action::AddAlbums(rows.iter().map(|r| r.id.clone()).collect()));
            }
        });
    }
    if !query.unknown.is_empty() {
        ui.label(RichText::new(format!("not understood: {} (hover over the search box for what works)", query.unknown.join(" "))).size(13.0).color(ink.warn));
    }
    let width = ui.available_width() - 14.0;
    let fixed: f32 = FIXED.iter().map(|c| c.2).sum::<f32>() + MARK_W;
    let artist_w = ((width - fixed) * 0.42).max(80.0);
    let album_w = (width - fixed - artist_w).max(80.0);
    // Column left edges: mark, artist, album, then the fixed ones.
    let mut lefts = vec![0.0, MARK_W, MARK_W + artist_w];
    let mut x = MARK_W + artist_w + album_w;
    for c in FIXED {
        lefts.push(x);
        x += c.2;
    }
    let widths: Vec<f32> = [MARK_W, artist_w, album_w].into_iter().chain(FIXED.iter().map(|c| c.2)).collect();

    // Headings: click to sort, again to turn the order around.
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let headings = [("artist", SortBy::Artist, false), ("album", SortBy::Album, false)].into_iter().chain(FIXED.iter().map(|c| (c.0, c.1, c.3)));
    for (k, (name, by, right)) in headings.enumerate() {
        let cell = egui::Rect::from_min_size(rect.min + vec2(lefts[k + 1], 0.0), vec2(widths[k + 1] - if right { RIGHT_PAD } else { 8.0 }, 22.0));
        let r = ui.interact(cell, egui::Id::unique(("album heading", k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let sorted = app.album_sort.0 == by;
        let arrow = match (sorted, app.album_sort.1) {
            (false, _) => "",
            (true, false) => " ▴",
            (true, true) => " ▾",
        };
        let color = if sorted || r.hovered() { ink.text } else { ink.muted };
        let (at, align) = if right { (cell.right_center(), Align2::RIGHT_CENTER) } else { (cell.left_center(), Align2::LEFT_CENTER) };
        ui.painter().text(at, align, format!("{}{arrow}", name.to_uppercase()), FontId::proportional(11.5), color);
        if r.clicked() {
            actions.push(Action::SortAlbums(by));
        }
    }
    ui.painter().hline(rect.left()..=rect.right(), rect.bottom(), Stroke::new(0.5, ink.faint));
    if rows.is_empty() {
        ui.add_space(12.0);
        muted(ui, "No albums match.");
        return;
    }
    let mods = ui.input(|i| i.modifiers);
    let current = app.current();
    ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW_H, rows.len(), |ui, range| {
        for n in range {
            let row = &rows[n];
            let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
            let p = ui.painter_at(rect);
            let selected = app.selected_albums.contains(&row.id);
            if selected {
                p.rect_filled(rect, 4.0, ink.chip);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, ink.hover);
            }
            let y = rect.center().y;
            let font = FontId::proportional(14.5);
            let small = FontId::proportional(12.5);
            let cell = |k: usize| rect.left() + lefts[k];
            let a = &app.index.artists[row.artist];
            let al = &a.albums[row.album];
            let ck = &app.index.clusters[a.cluster].key;
            let (mark, status) = album_status(app, ck, a, al);
            if let Some(m) = mark {
                m.paint(&p, egui::pos2(cell(0) + 5.0, y), 2.6, &ink);
            }
            let playing = current.is_some_and(|c| al.tracks.contains(&c));
            let text_at = |k: usize, s: String, f: &FontId, color| {
                let g = truncated(ui, s, f.clone(), color, widths[k] - 10.0);
                p.galley(egui::pos2(cell(k), y - g.size().y / 2.0), g, color);
            };
            text_at(1, row.artist_name.clone(), &font, if playing { ink.accent } else { ink.text });
            let title = if row.edited { format!("{} · edited", row.title) } else { row.title.clone() };
            text_at(2, title, &font, ink.text);
            text_at(3, row.year.map(|y| y.to_string()).unwrap_or_default(), &font, ink.muted);
            text_at(4, row.format.clone(), &small, if row.plays_on_device { ink.muted } else { ink.warn });
            let right = |k: usize, s: String| {
                p.text(egui::pos2(cell(k) + widths[k] - RIGHT_PAD, y), Align2::RIGHT_CENTER, s, small.clone(), ink.muted);
            };
            right(5, row.songs.to_string());
            right(6, text::bytes(row.bytes));
            let card_word = match mark {
                None => String::new(),
                Some(Mark::On) => "on card".into(),
                Some(Mark::Adding) => "at next sync".into(),
                Some(Mark::Removing) => "coming off".into(),
            };
            text_at(7, card_word, &small, if mark == Some(Mark::Removing) { ink.warn } else { ink.muted });
            if row.cover {
                icons::paint(&p, egui::Rect::from_center_size(egui::pos2(cell(8) + 8.0, y), vec2(14.0, 14.0)), Icon::Image, ink.muted);
            } else {
                text_at(8, "none".into(), &small, ink.warn);
            }
            let resp = if status.is_empty() { resp } else { resp.on_hover_text(status) };
            if resp.double_clicked() {
                actions.push(Action::Play(al.tracks.clone(), 0));
            } else if resp.clicked() && mods.command {
                actions.push(Action::ToggleAlbum(row.id.clone()));
            } else if resp.clicked() && mods.shift {
                let from = app.album_anchor.as_ref().and_then(|a| rows.iter().position(|r| &r.id == a)).unwrap_or(n);
                actions.push(Action::AddAlbums(rows[from.min(n)..=from.max(n)].iter().map(|r| r.id.clone()).collect()));
            } else if resp.clicked() {
                actions.push(Action::OnlyAlbum(row.id.clone()));
            }
            resp.context_menu(|ui| album_menu(app, ui, row, actions));
        }
    });
}

/// Right-click on an album row.
fn album_menu(app: &App, ui: &mut Ui, row: &AlbumRow, actions: &mut Vec<Action>) {
    let al = &app.index.artists[row.artist].albums[row.album];
    let mut item = |ui: &mut Ui, icon, text: &str, action: Action| {
        if widgets::icon_link(ui, icon, text).clicked() {
            actions.push(action);
            ui.close();
        }
    };
    item(ui, Icon::Play, "play", Action::Play(al.tracks.clone(), 0));
    item(ui, Icon::Rename, "edit tags", Action::EditTags(al.tracks.clone()));
    if row.card.chosen {
        item(ui, Icon::Remove, "take off card", Action::CardAlbums(vec![(row.artist, row.album)], false));
    } else {
        item(ui, Icon::Add, "send to card", Action::CardAlbums(vec![(row.artist, row.album)], true));
    }
    if !row.cover && app.busy.is_none() {
        item(ui, Icon::Image, "find cover", Action::FindCovers(vec![(row.artist, row.album)]));
    }
    if let Some(dir) = app.tracks[al.tracks[0]].path.parent()
        && widgets::icon_link(ui, Icon::Folder, "open folder").clicked()
    {
        crate::platform::reveal(dir);
        ui.close();
    }
}

/// Above the table while albums are selected: how much, and what to do with them.
fn selection_bar(app: &App, ui: &mut Ui, rows: &[AlbumRow], actions: &mut Vec<Action>) {
    let ink = ink(ui);
    if app.tag_edit.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        actions.push(Action::ClearAlbums);
    }
    let chosen: Vec<&AlbumRow> = rows.iter().filter(|r| app.selected_albums.contains(&r.id)).collect();
    let albums: Vec<(usize, usize)> = chosen.iter().map(|r| (r.artist, r.album)).collect();
    let songs = app.selected_album_songs(rows);
    let bytes: u64 = chosen.iter().map(|r| r.bytes).sum();
    let hidden = app.selected_albums.len() - chosen.len();
    theme::panel_frame(ui).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            let mut what = format!("{} selected · {} · {}", plural(chosen.len(), "album"), plural(songs.len(), "song"), text::bytes(bytes));
            if hidden > 0 {
                what.push_str(&format!(" (and {hidden} the search hides)"));
            }
            ui.label(RichText::new(what).color(ink.text));
            ui.add_space(8.0);
            if widgets::icon_link(ui, Icon::Add, "send to card").on_hover_text("copied at the next Sync").clicked() {
                actions.push(Action::CardAlbums(albums.clone(), true));
            }
            if widgets::icon_link(ui, Icon::Remove, "take off card").on_hover_text("taken off at the next Sync").clicked() {
                actions.push(Action::CardAlbums(albums.clone(), false));
            }
            if widgets::icon_link(ui, Icon::Rename, "edit tags").on_hover_text("set album artist, year… for all their songs").clicked() {
                actions.push(Action::EditTags(songs.clone()));
            }
            let coverless: Vec<(usize, usize)> = chosen.iter().filter(|r| !r.cover).map(|r| (r.artist, r.album)).collect();
            if !coverless.is_empty() && app.busy.is_none() && widgets::icon_link(ui, Icon::Image, format!("find covers ({})", coverless.len())).clicked() {
                actions.push(Action::FindCovers(coverless));
            }
            if widgets::icon_link(ui, Icon::Play, "play").clicked() {
                actions.push(Action::Play(songs, 0));
            }
            if widgets::icon_link(ui, Icon::Close, "clear").on_hover_text("or press Esc").clicked() {
                actions.push(Action::ClearAlbums);
            }
        });
        widgets::help(ui, "Click an album to select it, ⌘-click to add or remove one, shift-click for a run; double-click plays it.");
    });
    ui.add_space(6.0);
}
