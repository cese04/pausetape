//! Everything drawn on screen: the header and tabs, one module per tab, the player, the
//! folder pickers, and the shared theme, widgets and text helpers.

mod actions;
mod artists;
mod card;
pub mod folders;
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

use egui::{RichText, Ui};

use crate::app::{App, Tab};
use theme::ink;
use widgets::{hairline_progress, muted};

const MAX_W: f32 = 820.0;

const ROW_H: f32 = 22.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let pad = egui::Frame::new().fill(ink.bg).inner_margin(egui::Margin::symmetric(40, 18));
    egui::Panel::top("nav").frame(pad).show_separator_line(false).show(ui, |ui| header(app, ui));
    egui::Panel::bottom("player").frame(pad).show_separator_line(false).show(ui, |ui| player::show(app, ui));
    egui::CentralPanel::default().frame(pad.inner_margin(egui::Margin { left: 40, right: 40, top: 4, bottom: 0 })).show(ui, |ui| {
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

fn header(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::APP_NAME).size(24.0).color(ink.text));
        ui.add_space(24.0);
        for (t, name) in [
            (Tab::Folders, "folders"),
            (Tab::Library, "library"),
            (Tab::Artists, "artists"),
            (Tab::Card, "card"),
            (Tab::Playlists, "playlists"),
            (Tab::Settings, "settings"),
        ] {
            if widgets::tab(ui, name, app.tab == t).clicked() {
                app.tab = t;
            }
            ui.add_space(6.0);
        }
    });
    ui.horizontal_wrapped(|ui| {
        let albums: usize = app.index.artists.iter().map(|a| a.albums.len()).sum();
        let mut facts = vec![
            format!("{} tracks", app.tracks.len()),
            format!("{} artists", app.index.artists.len()),
            format!("{albums} albums"),
        ];
        if !app.index.suggestions.is_empty() {
            facts.push(format!("{} possible duplicate artists", app.index.suggestions.len()));
        }
        let dirty = app.dirty_count();
        if dirty > 0 {
            facts.push(format!("{dirty} tracks with corrected tags"));
        }
        muted(ui, facts.join(" · "));
        let spare = app.index.superseded();
        if !spare.is_empty() {
            let bytes: u64 = spare.iter().flat_map(|&(a, b)| &app.index.artists[a].albums[b].tracks).map(|&i| app.tracks[i].size).sum();
            muted(ui, "·");
            let copies = if spare.len() == 1 { "copy" } else { "copies" };
            let text = format!("{} album {copies} with a same-or-better copy ({})", spare.len(), text::bytes(bytes));
            let hint = "Copies whose songs are all in another copy of the album, in the same or higher quality. Show them in the library.";
            if widgets::link(ui, text).on_hover_text(hint).clicked() {
                app.tab = Tab::Library;
                app.duplicates_only = true;
            }
        }
        let repeats = app.index.repeats();
        if !repeats.is_empty() {
            let bytes: u64 = repeats.iter().map(|&(c, _)| app.tracks[c].size).sum();
            muted(ui, "·");
            let hint = "Songs kept twice in the same album, like the copies a backup tool leaves. Show them in the library.";
            if widgets::link(ui, format!("{} stored twice ({})", text::plural(repeats.len(), "song"), text::bytes(bytes))).on_hover_text(hint).clicked() {
                app.tab = Tab::Library;
                app.duplicates_only = true;
            }
        }
        if let Some(b) = &app.busy {
            ui.add_space(12.0);
            let txt = if b.total > 0 { format!("{} {}/{}", b.label, b.done, b.total) } else { format!("{} {}…", b.label, if b.done > 0 { b.done.to_string() } else { String::new() }) };
            ui.label(RichText::new(txt).color(ink.accent));
            if b.total > 0 {
                hairline_progress(ui, b.done as f32 / b.total as f32, 120.0);
            }
        }
    });
    for n in app.notices.iter().rev().take(1) {
        ui.label(RichText::new(n).size(12.5).color(ink.muted).italics());
    }
}
