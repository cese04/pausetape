//! Everything drawn on screen: the header and tabs, one module per tab, the player, the
//! folder pickers, and the shared theme, widgets and text helpers.

mod actions;
mod artists;
mod card;
pub mod folders;
pub mod icons;
mod library;
pub mod navigator;
mod player;
mod playlists;
#[cfg(debug_assertions)]
pub mod screenshots;
mod settings;
pub mod text;
pub mod theme;
pub mod widgets;

use egui::{RichText, Stroke, Ui};

use crate::app::{App, Tab};
use theme::ink;
use icons::Icon;
use widgets::hairline_progress;

const MAX_W: f32 = 820.0;

const ROW_H: f32 = 22.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let pad = egui::Frame::new().fill(ink.bg).inner_margin(egui::Margin::symmetric(40, 18));
    // The tab bar is tinted and ruled off, so the app's own frame never reads as content.
    let bar = egui::Frame::new().fill(ink.bar).inner_margin(egui::Margin::symmetric(32, 10));
    let r = egui::Panel::top("nav").frame(bar).show_separator_line(false).show(ui, |ui| header(app, ui));
    let edge = r.response.rect;
    ui.painter().hline(edge.x_range(), edge.bottom(), Stroke::new(1.0, ink.faint));
    if let Some(n) = app.notices.last().cloned() {
        let frame = egui::Frame::new().fill(ink.bg).inner_margin(egui::Margin { left: 40, right: 40, top: 10, bottom: 0 });
        egui::Panel::top("notice").frame(frame).show_separator_line(false).show(ui, |ui| notice(app, ui, &n));
    }
    egui::Panel::bottom("player").frame(pad).show_separator_line(false).show(ui, |ui| player::show(app, ui));
    egui::CentralPanel::default().frame(pad.inner_margin(egui::Margin { left: 40, right: 40, top: 16, bottom: 0 })).show(ui, |ui| {
        // The library uses the whole width (its A–Z rail belongs at the window's edge); the other
        // tabs are forms and read better at a fixed measure.
        if app.tab != Tab::Library {
            ui.set_max_width(ui.available_width().min(MAX_W));
        }
        match app.tab {
            Tab::Folders => folders::show(app, ui),
            Tab::Library => library::show(app, ui),
            Tab::Artists => artists::show(app, ui),
            Tab::Card => card::show(app, ui),
            Tab::Playlists => playlists::show(app, ui),
            Tab::Settings => settings::show(app, ui),
        }
    });
    folders::pickers(app, ui);
}

/// The tab bar: the app's name, the tabs, and what's running in the background.
fn header(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::APP_NAME).size(21.0).color(ink.text));
        ui.add_space(18.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        // In a narrow window only the open tab keeps its name; the others show their icon.
        let compact = ui.available_width() < 900.0;
        for (t, icon, name) in [
            (Tab::Folders, Icon::Folder, "folders"),
            (Tab::Library, Icon::Library, "library"),
            (Tab::Artists, Icon::Artists, "artists"),
            (Tab::Card, Icon::Card, "card"),
            (Tab::Playlists, Icon::Playlists, "playlists"),
            (Tab::Settings, Icon::Settings, "settings"),
        ] {
            let active = app.tab == t;
            let label = if compact && !active { "" } else { name };
            if widgets::nav_tab(ui, icon, label, active).on_hover_text(name).clicked() {
                app.tab = t;
                // The library tab always opens on the whole library.
                app.duplicates_only = false;
            }
        }
        if let Some(b) = &app.busy {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if b.total > 0 {
                    hairline_progress(ui, b.done as f32 / b.total as f32, 110.0);
                }
                let txt = if b.total > 0 { format!("{} {}/{}", b.label, b.done, b.total) } else { format!("{} {}…", b.label, if b.done > 0 { b.done.to_string() } else { String::new() }) };
                ui.add(egui::Label::new(RichText::new(txt).size(13.5).color(ink.accent)).truncate());
            });
        }
    });
}

/// The latest message from the app, until the next one or until closed.
fn notice(app: &mut App, ui: &mut Ui, text: &str) {
    let ink = ink(ui);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        icons::paint(ui.painter(), rect, Icon::Info, ink.muted);
        ui.allocate_ui(egui::vec2(ui.available_width() - 40.0, 0.0), |ui| {
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(ink.muted).italics()).wrap());
        });
        if widgets::icon_button(ui, Icon::Close).on_hover_text("dismiss").clicked() {
            app.notices.clear();
        }
    });
}
