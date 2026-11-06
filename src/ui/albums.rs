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
use super::layout::{S1, S2, row, row_wrapped};

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
        row(ui, |ui| {
            muted(ui, format!("{} match", plural(rows.len(), "album")));
            if widgets::icon_link(ui, Icon::Check, "Select them all").on_hover_text("to send them to the card, edit their tags or find their covers together").clicked() {
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
        widgets::name(ui, &r, egui::Role::ColumnHeader, &format!("sort by {name}"));
        widgets::focus_ring(ui, &r, cell, 2.0);
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
        ui.add_space(S2);
        muted(ui, "No albums match.");
        return;
    }
    let mods = ui.input(|i| i.modifiers);
    let current = app.current();
    // Only the rows in view are drawn, so a row the arrow keys moved to out of view is scrolled
    // to by its position: to the top edge going up, the bottom edge going down.
    let mut area = ScrollArea::vertical().auto_shrink(false);
    if let Some(n) = app.album_scroll_to.take() {
        let (pitch, was) = (ROW_H + ui.spacing().item_spacing.y, app.album_rows_shown.clone());
        if n <= was.start {
            area = area.vertical_scroll_offset(n as f32 * pitch);
        } else if n + 1 >= was.end {
            area = area.vertical_scroll_offset((n + 2).saturating_sub(was.len()) as f32 * pitch);
        }
    }
    let mut drawn = 0..0;
    area.show_rows(ui, ROW_H, rows.len(), |ui, range| {
        drawn = range.clone();
        for n in range {
            let row = &rows[n];
            let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
            let selected = app.selected_albums.contains(&row.id);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Row, true, selected, format!("{}, {}", row.artist_name, row.title)));
            widgets::focus_ring(ui, &resp, rect, 4.0);
            widgets::row_background(ui, &resp, rect, selected);
            let p = ui.painter_at(rect);
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
                Some(Mark::On) => "on the card".into(),
                Some(Mark::Adding) => "added at Sync".into(),
                Some(Mark::ByHand) => "by hand".into(),
                Some(Mark::Removing) => "taken off at Sync".into(),
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
            // On an album in the selection, the menu is for the whole selection.
            resp.context_menu(|ui| {
                let of = if selected && app.selected_albums.len() > 1 { super::menu::Of::Songs(app.selected_album_songs(&rows)) } else { super::menu::Of::Album(row.artist, row.album) };
                super::menu::music(app, ui, of, actions, |_, _| {});
            });
        }
    });
    app.album_rows_shown = drawn;
    album_keys(app, ui, &rows, actions);
}

/// The keyboard in the album table: ↑/↓ move the selection (with Shift, extend it), Enter plays,
/// ⌘A selects every row, ⌘I edits tags, ⌘D sends the albums to the card or takes them off.
fn album_keys(app: &App, ui: &Ui, rows: &[AlbumRow], actions: &mut Vec<Action>) {
    if rows.is_empty() || super::keys::taken(app, ui) {
        return;
    }
    let navigation = super::keys::navigation_free(app, ui);
    let at = app.album_anchor.as_ref().and_then(|a| rows.iter().position(|r| &r.id == a));
    for (key, step) in [(egui::Key::ArrowDown, 1isize), (egui::Key::ArrowUp, -1)] {
        if let Some(extend) = super::keys::nav_key(ui, key).filter(|_| navigation) {
            let next = match at {
                Some(i) => (i as isize + step).clamp(0, rows.len() as isize - 1) as usize,
                None if step > 0 => 0,
                None => rows.len() - 1,
            };
            actions.push(Action::AlbumCursor(rows[next].id.clone(), next, extend));
        }
    }
    let chosen: Vec<&AlbumRow> = rows.iter().filter(|r| app.selected_albums.contains(&r.id)).collect();
    if chosen.is_empty() {
        if super::keys::pressed(ui, &super::keys::SELECT_ALL) {
            actions.push(Action::AddAlbums(rows.iter().map(|r| r.id.clone()).collect()));
        }
        return;
    }
    if navigation && super::keys::nav_key(ui, egui::Key::Enter).is_some() {
        actions.push(Action::Play(app.selected_album_songs(rows), 0));
    }
    if super::keys::pressed(ui, &super::keys::SELECT_ALL) {
        actions.push(Action::AddAlbums(rows.iter().map(|r| r.id.clone()).collect()));
    }
    if super::keys::pressed(ui, &super::keys::EDIT_TAGS) {
        actions.push(Action::EditTags(app.selected_album_songs(rows)));
    }
    if super::keys::pressed(ui, &super::keys::CARD) {
        actions.push(Action::ToggleCard(chosen.iter().map(|r| (r.artist, r.album)).collect()));
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
    theme::panel_frame(ui).inner_margin(egui::Margin::symmetric(S2 as i8, S1 as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        row_wrapped(ui, |ui| {
            let mut what = format!("{} selected · {} · {}", plural(chosen.len(), "album"), plural(songs.len(), "song"), text::bytes(bytes));
            if hidden > 0 {
                what.push_str(&format!(" (and {hidden} the search hides)"));
            }
            ui.label(RichText::new(what).color(ink.text));
            ui.add_space(S1);
            if widgets::icon_link(ui, Icon::Add, "Send to the card").on_hover_text(format!("copied at the next Sync ({} sends or takes off)", ui.ctx().format_shortcut(&super::keys::CARD))).clicked() {
                actions.push(Action::CardAlbums(albums.clone(), true));
            }
            if widgets::icon_link(ui, Icon::Remove, "Take off the card").on_hover_text("taken off at the next Sync").clicked() {
                actions.push(Action::CardAlbums(albums.clone(), false));
            }
            if widgets::icon_link(ui, Icon::Rename, "Edit tags").on_hover_text(format!("set album artist, year… for all their songs ({})", ui.ctx().format_shortcut(&super::keys::EDIT_TAGS))).clicked() {
                actions.push(Action::EditTags(songs.clone()));
            }
            let coverless: Vec<(usize, usize)> = chosen.iter().filter(|r| !r.cover).map(|r| (r.artist, r.album)).collect();
            if !coverless.is_empty() && app.busy.is_none() && widgets::icon_link(ui, Icon::Image, format!("Find covers ({})", coverless.len())).clicked() {
                actions.push(Action::FindCovers(coverless));
            }
            if widgets::icon_link(ui, Icon::Play, "Play").clicked() {
                actions.push(Action::Play(songs, 0));
            }
            if widgets::icon_link(ui, Icon::Close, "Clear").on_hover_text("or press Esc").clicked() {
                actions.push(Action::ClearAlbums);
            }
        });
        widgets::help(ui, "Click an album to select it, ⌘-click to add or remove one, shift-click for a run; double-click plays it.");
    });
    ui.add_space(S1);
}
