//! Everything drawn on screen: the header and tabs, one module per tab, the player, the
//! folder pickers, and the shared theme, widgets and text helpers.

mod actions;
mod albums;
mod artist_panel;
mod card;
mod cassette;
mod confirm;
pub mod folders;
mod keys;
mod menu;
pub mod icons;
pub mod layout;
mod library;
pub mod navigator;
mod player;
mod player_test;
mod playlists;
#[cfg(debug_assertions)]
pub mod screenshots;
mod settings;
mod tag_editor;
pub mod text;
pub mod theme;
pub mod widgets;

use egui::{RichText, Stroke, Ui};

use crate::app::{App, Tab};
use text::plural;
use theme::ink;
use icons::Icon;
use widgets::hairline_progress;
use layout::{CONTROL, ICON, S1, S2, S3, S4, S5, margin, row};

const MAX_W: f32 = 820.0;

const ROW_H: f32 = layout::ROW;

/// The panels, top to bottom: the tab bar; a strip for work under way and the latest notice;
/// the open tab; the player. The tab bar and the player are the app's frame: tinted and ruled
/// off, so they never read as content. Everything inside keeps to the page's side margin.
pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    keys::global(app, ui);
    let chrome = |x: f32, y: f32| egui::Frame::new().fill(ink.bar).inner_margin(margin(x, y));
    let r = egui::Panel::top("nav").frame(chrome(S4, S1)).show_separator_line(false).show(ui, |ui| header(app, ui));
    let edge = r.response.rect;
    ui.painter().hline(edge.x_range(), edge.bottom(), Stroke::new(1.0, ink.faint));
    // The status bar, at the very bottom: always there and always one line, so neither work
    // starting nor a notice ever moves the page.
    let r = egui::Panel::bottom("status").frame(chrome(S5, S1 / 2.0)).exact_size(CONTROL + S1).show_separator_line(false).show(ui, |ui| status_bar(app, ui));
    let edge = r.response.rect;
    ui.painter().hline(edge.x_range(), edge.top(), Stroke::new(1.0, ink.faint));
    let r = egui::Panel::bottom("player").frame(chrome(S5, S2)).show_separator_line(false).show(ui, |ui| player::show(app, ui));
    let edge = r.response.rect;
    ui.painter().hline(edge.x_range(), edge.top(), Stroke::new(1.0, ink.faint));
    let page = egui::Frame::new().fill(ink.bg).inner_margin(egui::Margin { left: S5 as i8, right: S5 as i8, top: S2 as i8, bottom: S2 as i8 });
    egui::CentralPanel::default().frame(page).show(ui, |ui| {
        // The library uses the whole width (its A–Z rail belongs at the window's edge); the other
        // tabs are forms and read better at a fixed measure.
        if app.tab != Tab::Library {
            ui.set_max_width(ui.available_width().min(MAX_W));
        }
        match app.tab {
            Tab::Folders => folders::show(app, ui),
            Tab::Library => library::show(app, ui),
            Tab::Card => card::show(app, ui),
            Tab::Playlists => playlists::show(app, ui),
            Tab::Settings => settings::show(app, ui),
        }
    });
    folders::pickers(app, ui);
    tag_editor::dialog(app, ui);
    card::take_over_dialog(app, ui);
    player_test::dialog(app, ui);
    // ⌘F is for the search box of the tab it opened; if that tab didn't draw one, it lapses.
    app.focus_search = false;
    drop_to_add(app, ui);
    card::sync_confirm_dialog(app, ui);
    confirm::dialog(app, ui);
    folders::add_dialog(app, ui);
}

/// A folder dropped on the window is music to add, as "add music" would: while it's held over
/// the window, the page says so; once dropped, it's checked against the library.
fn drop_to_add(app: &mut App, ui: &mut Ui) {
    let (hovering, dropped) = ui.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect::<Vec<_>>()));
    if hovering {
        let ink = ink(ui);
        let area = ui.ctx().content_rect().shrink(S2);
        let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop to add")));
        p.rect_filled(area, 8.0, ink.bg.gamma_multiply(0.92));
        p.rect_filled(area, 8.0, ink.accent.gamma_multiply(0.10));
        p.rect_stroke(area, 8.0, Stroke::new(1.5, ink.accent), egui::StrokeKind::Inside);
        let what = if app.saved.source.is_some() { "Drop a folder to add its music to your library" } else { "Choose your music folder first (folders tab), then drop music to add" };
        p.text(area.center(), egui::Align2::CENTER_CENTER, what, egui::FontId::proportional(20.0), ink.text);
        p.text(area.center() + egui::vec2(0.0, S4), egui::Align2::CENTER_CENTER, "It's checked against the library first; nothing is copied until you say so.", egui::FontId::proportional(14.0), ink.muted);
    }
    let Some(first) = dropped.first() else { return };
    if app.dialog_open() {
        app.notice("close the dialog first, then drop it again");
    } else if let Some(b) = &app.busy {
        app.notice(format!("wait until {} is done, then drop it again", b.label));
    } else if !first.is_dir() {
        app.notice("drop the folder the music is in (an album, or a folder of albums), not single files");
    } else {
        if dropped.len() > 1 {
            app.notice("one folder at a time: checking the first");
        }
        app.tab = Tab::Library;
        app.check_folder(first.clone());
    }
}

/// The tab bar: the app's name, the tabs, and what's running in the background.
fn header(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    row(ui, |ui| {
        cassette::mark(ui, 18.0);
        ui.label(RichText::new(crate::APP_NAME).size(21.0).color(ink.text));
        ui.add_space(S3);
        // In a narrow window only the open tab keeps its name; the others show their icon.
        let compact = ui.available_width() < 900.0;
        for (t, icon, name) in [
            (Tab::Folders, Icon::Folder, "Folders"),
            (Tab::Library, Icon::Library, "Library"),
            (Tab::Card, Icon::Card, "Card"),
            (Tab::Playlists, Icon::Playlists, "Playlists"),
            (Tab::Settings, Icon::Settings, "Settings"),
        ] {
            let active = app.tab == t;
            if widgets::nav_tab(ui, icon, name, !compact || active, active).on_hover_text(name).clicked() {
                app.tab = t;
                // The library tab always opens on the whole library.
                app.duplicates_only = false;
                app.look_alikes_only = false;
            }
        }
    });
}

/// One line at the foot of the window. On the left, the library in figures and what's selected;
/// on the right, work under way (how far, time left, Stop) or else the latest notice, with
/// *Undo* when it offers one.
fn status_bar(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    ui.set_height(CONTROL + S1);
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = S2;
        let albums: usize = app.index.artists.iter().map(|a| a.albums.len()).sum();
        let mut facts = if app.tracks.is_empty() { String::new() } else { format!("{} · {} · {}", plural(app.tracks.len(), "song"), plural(app.index.artists.len(), "artist"), plural(albums, "album")) };
        if !app.selected.is_empty() {
            facts.push_str(&format!(" · {} selected", plural(app.selected.len(), "song")));
        } else if !app.selected_albums.is_empty() {
            facts.push_str(&format!(" · {} selected", plural(app.selected_albums.len(), "album")));
        }
        ui.label(RichText::new(facts).size(13.0).color(ink.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if app.busy.is_some() {
                job(app, ui);
            } else if let Some(n) = app.notices.last().cloned() {
                notice(app, ui, &n);
            }
        });
    });
}

/// What's running: the reels turning, what it is, how far along, the time left at the pace so
/// far, and a way to stop it. Laid out right to left, from the Stop button.
fn job(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let Some(b) = &app.busy else { return };
    if let Some(stop) = b.stop.clone().filter(|_| !b.stopping()) {
        let hint = "Finishes the file it's on and stops there. What's done stays done: a stopped write keeps what it copied, and the next Sync carries on.";
        if widgets::button_with(ui, Icon::Close, "Stop").on_hover_text(hint).clicked() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    if b.total > 0 {
        hairline_progress(ui, b.done as f32 / b.total as f32, (ui.available_width() / 4.0).clamp(60.0, 200.0));
    }
    let mut facts = match (b.done, b.total) {
        (_, 0) if b.done > 0 => b.done.to_string(),
        (_, 0) => String::new(),
        (d, t) => format!("{d} of {t}"),
    };
    if b.stopping() {
        facts = "stopping after the file it's on…".into();
    } else if let Some(left) = b.time_left() {
        facts.push_str(&format!(" · {} left", text::time_left(left)));
    }
    ui.label(RichText::new(facts).size(13.0).color(ink.muted));
    ui.label(RichText::new(&b.label).size(13.5).color(ink.text));
    cassette::spinner(ui, ICON);
}

/// The latest message from the app, until the next one or until closed: on one line, shortened
/// with "…" if it must be (all of it on hover). Laid out right to left, from the close button.
fn notice(app: &mut App, ui: &mut Ui, text: &str) {
    let ink = ink(ui);
    if widgets::icon_button(ui, Icon::Close, "Dismiss").clicked() {
        app.notices.clear();
        app.undo_notice = None;
    }
    if app.notice_offers_undo() && widgets::icon_link(ui, Icon::Back, format!("Undo ({})", ui.ctx().format_shortcut(&keys::UNDO))).clicked() {
        app.undo_last();
    }
    widgets::fitted(ui, RichText::new(text).size(13.0).color(ink.text), ui.available_width() - ICON - S2, egui::Sense::hover()).on_hover_text(text);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ICON, ICON), egui::Sense::hover());
    icons::paint(ui.painter(), rect.shrink(1.0), Icon::Info, ink.muted);
}
