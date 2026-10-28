//! The settings tab.

use egui::{RichText, Ui};

use super::theme::ink;
use super::widgets::{caps, link, muted};
use crate::app::{App, ThemePref};

pub fn show(app: &mut App, ui: &mut Ui) {
    egui::Grid::new("settings").num_columns(2).spacing([24.0, 10.0]).show(ui, |ui| {
        caps(ui, "musicbrainz contact");
        ui.vertical(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.saved.mb_contact).hint_text("email or URL (optional)").desired_width(320.0));
            muted(ui, "MusicBrainz asks API clients to identify themselves; it is sent in the User-Agent.");
        });
        ui.end_row();
        caps(ui, "appearance");
        ui.horizontal(|ui| {
            let mut changed = false;
            changed |= ui.radio_value(&mut app.saved.theme, ThemePref::System, "system").changed();
            changed |= ui.radio_value(&mut app.saved.theme, ThemePref::Light, "paper").changed();
            changed |= ui.radio_value(&mut app.saved.theme, ThemePref::Dark, "ink").changed();
            if changed {
                app.apply_theme();
            }
        });
        ui.end_row();
        caps(ui, "artist decisions");
        ui.horizontal(|ui| {
            let p = &app.saved.prefs;
            muted(ui, format!("{} merges · {} kept apart · {} names", p.merges.len(), p.rejected.len(), p.names.len()));
            if link(ui, "forget all").clicked() {
                app.saved.prefs = Default::default();
                app.rebuild();
            }
        });
        ui.end_row();
        caps(ui, "ffmpeg");
        ui.vertical(|ui| {
            match crate::audio::ffmpeg() {
                Some(p) => {
                    if crate::audio::ffmpeg_is_bundled() {
                        ui.label("included with the app (LGPL v2.1; licence and source details in the app's Resources folder)");
                    } else {
                        ui.label(format!("found at {}", p.display()));
                    }
                    muted(ui, "Used to play what the built-in decoder can't: Dolby E-AC-3/AC-3 (e.g. Atmos releases), TrueHD, DTS, Opus, APE, WavPack, WMA.");
                }
                None => {
                    ui.label(RichText::new("not found").color(ink(ui).accent));
                    muted(ui, "Install it (brew install ffmpeg) to play Dolby, Opus, APE, WavPack and WMA files here.");
                }
            }
        });
        ui.end_row();
        caps(ui, "library database");
        ui.vertical(|ui| {
            match &app.db {
                Some(db) => {
                    let (tracks, edits) = db.counts();
                    muted(ui, format!("{tracks} songs remembered · {edits} unsaved corrections"));
                }
                None => {
                    muted(ui, "not available; every scan reads all files");
                }
            }
            ui.add(egui::Label::new(RichText::new(crate::library::db::default_path().display().to_string()).size(12.0).color(ink(ui).muted)).truncate());
            muted(ui, "Rescans only open files that are new or changed since they were last read.");
            ui.horizontal(|ui| {
                if app.busy.is_none() && link(ui, "read every file again").on_hover_text("for tags edited by tools that don't update the file's date").clicked() {
                    app.read_all_again();
                }
                if link(ui, "forget stored tags").on_hover_text("unsaved corrections are kept").clicked()
                    && let Some(db) = &mut app.db
                {
                    let _ = db.clear();
                }
            });
        });
        ui.end_row();
    });
}
