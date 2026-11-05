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
use theme::ink;
use icons::Icon;
use widgets::hairline_progress;
use layout::{ICON, S1, S2, S3, S4, S5, margin, row};

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
    let notice_text = app.notices.last().cloned();
    if app.busy.is_some() || notice_text.is_some() {
        let frame = egui::Frame::new().fill(ink.bg).inner_margin(egui::Margin { left: S5 as i8, right: S5 as i8, top: S1 as i8, bottom: 0 });
        egui::Panel::top("status").frame(frame).show_separator_line(false).show(ui, |ui| {
            job(app, ui);
            if let Some(n) = &notice_text {
                notice(app, ui, n);
            }
        });
    }
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
    card::sync_confirm_dialog(app, ui);
    confirm::dialog(app, ui);
    folders::add_dialog(app, ui);
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
            (Tab::Folders, Icon::Folder, "folders"),
            (Tab::Library, Icon::Library, "library"),
            (Tab::Card, Icon::Card, "card"),
            (Tab::Playlists, Icon::Playlists, "playlists"),
            (Tab::Settings, Icon::Settings, "settings"),
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

/// What's running: how far along, the time left at the pace so far, and a way to stop it.
fn job(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let Some(b) = &app.busy else { return };
    row(ui, |ui| {
        ui.spacing_mut().item_spacing.x = S2;
        cassette::spinner(ui, ICON);
        ui.label(RichText::new(&b.label).size(14.5).color(ink.text));
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
        ui.label(RichText::new(facts).size(13.5).color(ink.muted));
        if b.total > 0 {
            hairline_progress(ui, b.done as f32 / b.total as f32, (ui.available_width() - 120.0).clamp(60.0, 260.0));
        }
        if let Some(stop) = b.stop.clone().filter(|_| !b.stopping()) {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = "Finishes the file it's on and stops there. What's done stays done: a stopped write keeps what it copied, and the next Sync carries on.";
                if widgets::button_with(ui, Icon::Close, "Stop").on_hover_text(hint).clicked() {
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });
        }
    });
}

/// The latest message from the app, until the next one or until closed.
fn notice(app: &mut App, ui: &mut Ui, text: &str) {
    let ink = ink(ui);
    row(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ICON, ICON), egui::Sense::hover());
        icons::paint(ui.painter(), rect.shrink(1.0), Icon::Info, ink.muted);
        let offers_undo = app.notice_offers_undo();
        let undo_w = if offers_undo { widgets::text_width(ui, "undo (⌘Z)", 13.5) + S4 } else { 0.0 };
        ui.allocate_ui(egui::vec2(ui.available_width() - S5 - undo_w, 0.0), |ui| {
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(ink.muted).italics()).wrap());
        });
        if offers_undo && widgets::icon_link(ui, Icon::Back, "undo (⌘Z)").clicked() {
            app.undo_last();
        }
        if widgets::icon_button(ui, Icon::Close, "dismiss").clicked() {
            app.notices.clear();
        }
    });
}
