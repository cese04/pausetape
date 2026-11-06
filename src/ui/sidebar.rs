//! The sidebar: the whole app in one column, as Calibre's tag browser has it. The library and its
//! two views; what's worth a look (Review), each a view of the library; saved searches;
//! playlists; the card; settings at the foot. In a narrow window it keeps only its icons.

use egui::{Align, Layout, Rect, RichText, ScrollArea, Sense, Ui, vec2};

use super::icons::{self, Icon};
use super::layout::{ICON, ROW, S1, S2, S3};
use super::text;
use super::theme::ink;
use super::widgets;
use crate::app::{App, Tab};

/// The sidebar's width; in a window narrower than `NARROW_BELOW`, only its icons.
pub const WIDE: f32 = 224.0;
pub const NARROW: f32 = 48.0;
const NARROW_BELOW: f32 = 900.0;

pub fn narrow(ctx: &egui::Context) -> bool {
    ctx.content_rect().width() < NARROW_BELOW
}

/// Where a click in the sidebar goes.
enum Go {
    Albums,
    Artists,
    Genres,
    /// The library through a search ("cover:no").
    Search(String),
    Duplicates,
    LookAlikes,
    Suggestions,
    Playlist(usize),
    NewPlaylist,
    /// Songs dropped on a playlist, or on "New playlist".
    AddTo(usize, Vec<usize>),
    NewWith(Vec<usize>),
    Card(usize),
    Settings,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let narrow = narrow(ui.ctx());
    let mut go = None;
    let mut forget = None;
    // Settings stay at the foot.
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.add_space(S1);
        if item(ui, Icon::Settings, "Settings", None, app.tab == Tab::Settings, narrow).clicked() {
            go = Some(Go::Settings);
        }
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                library(app, ui, narrow, &mut go);
                review(app, ui, narrow, &mut go);
                searches(app, ui, narrow, &mut go, &mut forget);
                playlists(app, ui, narrow, &mut go);
                card(app, ui, narrow, &mut go);
            });
        });
    });
    if let Some(i) = forget {
        app.saved.searches.remove(i);
    }
    let Some(go) = go else { return };
    // Every view of the library starts from the whole of it.
    let library = |app: &mut App, filter: String, table: Option<bool>| {
        app.tab = Tab::Library;
        app.filter = filter;
        app.duplicates_only = false;
        app.look_alikes_only = false;
        if let Some(t) = table {
            app.saved.album_table = t;
        }
    };
    match go {
        Go::Albums => library(app, String::new(), Some(true)),
        Go::Artists => library(app, String::new(), Some(false)),
        Go::Genres => app.tab = Tab::Genres,
        Go::Search(s) => {
            let again = app.tab == Tab::Library && app.filter.trim() == s;
            library(app, if again { String::new() } else { s }, None);
        }
        Go::Duplicates => {
            library(app, String::new(), None);
            app.duplicates_only = true;
        }
        Go::LookAlikes => {
            library(app, String::new(), Some(false));
            app.look_alikes_only = true;
        }
        Go::Playlist(i) => {
            app.tab = Tab::Playlists;
            app.active_playlist = Some(i);
        }
        Go::NewPlaylist => {
            app.tab = Tab::Playlists;
            app.new_playlist();
        }
        Go::AddTo(i, songs) => app.add_songs_to_playlist(i, &songs),
        Go::NewWith(songs) => {
            app.new_playlist();
            if let Some(i) = app.active_playlist {
                app.add_songs_to_playlist(i, &songs);
            }
        }
        Go::Card(i) => {
            app.tab = Tab::Card;
            if i != app.saved.active_write {
                app.select_write(i);
            }
        }
        Go::Settings => app.tab = Tab::Settings,
        Go::Suggestions => app.tab = Tab::Suggestions,
    }
}

/// A section's heading in small capitals; in the narrow sidebar, a rule.
fn heading(ui: &mut Ui, title: &str, narrow: bool) {
    ui.add_space(S2);
    if narrow {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), S1), Sense::hover());
        ui.painter().hline(rect.shrink2(vec2(S1, 0.0)).x_range(), rect.center().y, egui::Stroke::new(1.0, ink(ui).faint));
    } else {
        ui.horizontal(|ui| {
            ui.add_space(S2);
            widgets::caps(ui, title);
        });
    }
    ui.add_space(S1 / 2.0);
}

/// One entry: icon, name, and a count at the right end; filled in when it's what's shown.
/// Narrow, the icon alone, its name on hover.
fn item(ui: &mut Ui, icon: Icon, name: &str, count: Option<String>, active: bool, narrow: bool) -> egui::Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW + S1 / 2.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, active, name));
    let rect = rect.shrink2(vec2(S1 / 2.0, 0.0));
    widgets::focus_ring(ui, &resp, rect, 4.0);
    widgets::row_background(ui, &resp, rect, active);
    let color = if active { ink.text } else { ink.muted };
    let y = rect.center().y;
    let icon_x = if narrow { rect.center().x - ICON / 2.0 } else { rect.left() + S2 };
    icons::paint(ui.painter(), Rect::from_min_size(egui::pos2(icon_x, y - ICON / 2.0), vec2(ICON, ICON)).shrink(1.0), icon, if active { ink.accent } else { ink.muted });
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if narrow {
        let tip = match &count {
            Some(c) => format!("{name} ({c})"),
            None => name.to_string(),
        };
        return resp.on_hover_text(tip);
    }
    let count_w = count.as_ref().map(|c| widgets::text_width(ui, c, 12.5) + S2).unwrap_or(0.0);
    let words_x = rect.left() + S2 + ICON + S1;
    let g = widgets::truncated(ui, name.to_string(), egui::FontId::proportional(14.5), color, rect.right() - words_x - count_w - S1);
    let shortened = g.size().x + 1.0 < widgets::text_width(ui, name, 14.5);
    ui.painter().galley(egui::pos2(words_x, y - g.size().y / 2.0), g, color);
    if let Some(c) = count {
        ui.painter().text(egui::pos2(rect.right() - S1, y), egui::Align2::RIGHT_CENTER, c, egui::FontId::proportional(12.5), ink.muted);
    }
    if shortened { resp.on_hover_text(name) } else { resp }
}

/// The music folder (a menu to change it, read it again, add music), then the two views.
fn library(app: &mut App, ui: &mut Ui, narrow: bool, go: &mut Option<Go>) {
    let ink = ink(ui);
    ui.add_space(S1);
    let name = app.saved.source.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "No music folder".into());
    ui.horizontal(|ui| {
        ui.add_space(if narrow { S1 } else { S2 });
        let label = if narrow { "▾".to_string() } else { format!("{} ▾", text::shorten(&name, 22)) };
        let where_ = app.saved.source.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let button = ui.add(egui::Button::new(RichText::new(label).size(15.5).color(if app.source_missing { ink.warn } else { ink.text })).frame(false));
        egui::Popup::menu(&button).show(|ui| {
            if widgets::icon_link(ui, Icon::Folder, "Change folder…").clicked() {
                app.picker = Some(super::folders::Picker::Source(super::navigator::Navigator::new(app.saved.source.as_deref())));
                ui.close();
            }
            if app.busy.is_none() && widgets::icon_link(ui, Icon::Rescan, "Rescan").on_hover_text("pick up files added, changed or removed since the folder was last read").clicked() {
                app.scan();
                ui.close();
            }
            if app.busy.is_none() && widgets::icon_link(ui, Icon::Add, "Add music…").on_hover_text("copy new albums from Downloads or another drive into your music folder").clicked() {
                app.picker = Some(super::folders::Picker::AddFrom(super::navigator::Navigator::new(super::folders::dirs_downloads().as_deref())));
                ui.close();
            }
            if let Some(p) = &app.saved.source {
                ui.separator();
                widgets::help(ui, p.display().to_string());
            }
        });
        button
        .on_hover_text(if app.source_missing { format!("{where_}\nCan't reach this folder: is its drive connected?") } else { where_ });
    });
    let in_library = app.tab == Tab::Library && app.filter.trim().is_empty() && !app.duplicates_only && !app.look_alikes_only;
    let albums: usize = app.index.artists.iter().map(|a| a.albums.len()).sum();
    if item(ui, Icon::Library, "Albums", Some(text::count(albums)), in_library && app.saved.album_table, narrow).on_hover_text("a table of albums: sort by any column, select several").clicked() {
        *go = Some(Go::Albums);
    }
    if item(ui, Icon::Artists, "Artists", Some(text::count(app.index.artists.len())), in_library && !app.saved.album_table, narrow).on_hover_text("albums by artist, with their songs").clicked() {
        *go = Some(Go::Artists);
    }
    // A genre's artists and albums are the library by artist, searched by genre: still "Genres".
    let in_genres = app.tab == Tab::Genres || app.tab == Tab::Library && app.filter.trim().starts_with("genre:");
    let genres = app.genre_index().genres.len();
    if item(ui, Icon::Discover, "Genres", Some(text::count(genres)), in_genres, narrow).on_hover_text("every genre, and each one's artists and albums").clicked() {
        *go = Some(Go::Genres);
    }
}

/// What's worth a look, each with how many: only what there is.
fn review(app: &mut App, ui: &mut Ui, narrow: bool, go: &mut Option<Go>) {
    let searching = |s: &str| app.tab == Tab::Library && app.filter.trim() == s;
    let mut rows: Vec<(Icon, String, usize, Go, bool)> = Vec::new();
    let coverless = app.albums_without_cover().len();
    rows.push((Icon::Image, "Without a cover".into(), coverless, Go::Search("cover:no".into()), searching("cover:no")));
    let duplicates = app.index.superseded().len() + app.index.artists.iter().flat_map(|a| &a.albums).filter(|al| !al.repeats.is_empty()).count();
    rows.push((Icon::Copy, "Duplicates".into(), duplicates, Go::Duplicates, app.tab == Tab::Library && app.duplicates_only));
    rows.push((Icon::Artists, "Artists that look alike".into(), app.index.suggestions.len(), Go::LookAlikes, app.tab == Tab::Library && app.look_alikes_only));
    let damaged = app.tracks.iter().filter(|t| t.damaged).count();
    rows.push((Icon::Remove, "Damaged files".into(), damaged, Go::Search("damaged:yes".into()), searching("damaged:yes")));
    let edited = app.dirty_count();
    rows.push((Icon::Rename, "Corrected, not in the files".into(), edited, Go::Search("edited:yes".into()), searching("edited:yes")));
    rows.push((Icon::Search, "Suggested tags".into(), app.lookups.len(), Go::Suggestions, app.tab == Tab::Suggestions));
    let rows: Vec<_> = rows.into_iter().filter(|r| r.2 > 0).collect();
    if rows.is_empty() {
        return;
    }
    heading(ui, "review", narrow);
    for (icon, name, n, target, active) in rows {
        if item(ui, icon, &name, Some(text::count(n)), active, narrow).clicked() {
            *go = Some(target);
        }
    }
}

/// Searches kept for later; right-click to forget one.
fn searches(app: &mut App, ui: &mut Ui, narrow: bool, go: &mut Option<Go>, forget: &mut Option<usize>) {
    if app.saved.searches.is_empty() {
        return;
    }
    heading(ui, "searches", narrow);
    for (i, s) in app.saved.searches.iter().enumerate() {
        let active = app.tab == Tab::Library && app.filter.trim() == s;
        let r = item(ui, Icon::Search, s, None, active, narrow);
        if r.clicked() {
            *go = Some(Go::Search(s.clone()));
        }
        r.context_menu(|ui| {
            if widgets::icon_link(ui, Icon::Close, "Forget this search").clicked() {
                *forget = Some(i);
                ui.close();
            }
        });
    }
}

fn playlists(app: &mut App, ui: &mut Ui, narrow: bool, go: &mut Option<Go>) {
    heading(ui, "playlists", narrow);
    for (i, p) in app.saved.playlists.iter().enumerate() {
        let active = app.tab == Tab::Playlists && app.active_playlist == Some(i);
        let r = item(ui, Icon::Playlists, &p.name, Some(text::count(p.tracks.len())), active, narrow);
        if let Some(songs) = super::drag::songs_to(ui, &r, r.rect) {
            *go = Some(Go::AddTo(i, songs));
        } else if r.clicked() {
            *go = Some(Go::Playlist(i));
        }
    }
    let r = item(ui, Icon::Add, "New playlist", None, false, narrow).on_hover_text("or drop songs here to make one with them");
    if let Some(songs) = super::drag::songs_to(ui, &r, r.rect) {
        *go = Some(Go::NewWith(songs));
    } else if r.clicked() {
        *go = Some(Go::NewPlaylist);
    }
}

/// The card setups (the one, while there's one), with what the next Sync does.
fn card(app: &mut App, ui: &mut Ui, narrow: bool, go: &mut Option<Go>) {
    heading(ui, "card", narrow);
    let pending = super::card::pending_count(app);
    for (i, w) in app.saved.writes.iter().enumerate() {
        let active = app.tab == Tab::Card && app.saved.active_write == i;
        let count = (i == app.saved.active_write && pending > 0).then(|| format!("{} to sync", text::count(pending)));
        let r = item(ui, Icon::Card, &w.name, count, active, narrow);
        let connected = w.dest.as_deref().is_some_and(|d| d.exists());
        let r = r.on_hover_text(if connected { "connected" } else { "not connected" });
        if r.clicked() {
            *go = Some(Go::Card(i));
        }
    }
    if !narrow && app.saved.writes.len() == 1 && app.write().dest.is_none() {
        ui.horizontal(|ui| {
            ui.add_space(S3 + S1);
            widgets::help(ui, "no card chosen yet");
        });
    }
}
