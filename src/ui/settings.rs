//! The settings tab.

use egui::{RichText, Ui};

use super::icons::Icon;
use super::theme::{Accent, ink};
use super::widgets::{self, help, icon_link, setting};
use crate::app::{App, ThemePref};

pub fn show(app: &mut App, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        widgets::section(ui, Icon::Settings, "Appearance");
        ui.add_space(8.0);
        setting(ui, "Colours", |ui| {
            ui.horizontal(|ui| {
                let mut changed = false;
                changed |= ui.radio_value(&mut app.saved.theme, ThemePref::System, "like the system").changed();
                changed |= ui.radio_value(&mut app.saved.theme, ThemePref::Light, "paper").changed();
                changed |= ui.radio_value(&mut app.saved.theme, ThemePref::Dark, "ink").changed();
                if changed {
                    app.apply_theme();
                }
            });
        });
        setting(ui, "Accent", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let dark = ui.visuals().dark_mode;
                for accent in Accent::ALL {
                    if widgets::swatch(ui, accent.colour(dark), app.saved.accent == accent).on_hover_text(accent.name()).clicked() {
                        app.saved.accent = accent;
                        app.apply_theme();
                    }
                }
                ui.add_space(4.0);
                ui.label(RichText::new(app.saved.accent.name()).color(ink(ui).muted));
            });
            help(ui, "Marks the open tab, the main button, progress and what's playing. Warnings stay red.");
        });

        ui.add_space(10.0);
        widgets::section(ui, Icon::Library, "Library");
        ui.add_space(8.0);
        setting(ui, "Library database", |ui| {
            match &app.db {
                Some(db) => {
                    let (tracks, edits) = db.counts();
                    ui.label(format!("{tracks} songs remembered · {edits} unsaved corrections"));
                }
                None => {
                    ui.label("not available; every scan reads all files");
                }
            }
            help(ui, "Rescans only open files that are new or changed since they were last read.");
            ui.add(egui::Label::new(RichText::new(crate::library::db::default_path().display().to_string()).size(12.0).color(ink(ui).muted)).truncate());
            ui.horizontal(|ui| {
                if app.busy.is_none() && icon_link(ui, Icon::Rescan, "read every file again").on_hover_text("for tags edited by tools that don't update the file's date").clicked() {
                    app.read_all_again();
                }
                if icon_link(ui, Icon::Trash, "forget stored tags").on_hover_text("unsaved corrections are kept").clicked()
                    && let Some(db) = &mut app.db
                {
                    let _ = db.clear();
                }
            });
        });
        setting(ui, "Artist decisions", |ui| {
            let p = &app.saved.prefs;
            ui.label(format!("{} merged · {} kept apart · {} renamed", p.merges.len(), p.rejected.len(), p.names.len()));
            help(ui, "What you decided in the artists tab about names that look alike.");
            if icon_link(ui, Icon::Trash, "forget them all").clicked() {
                app.saved.prefs = Default::default();
                app.rebuild();
            }
        });

        ui.add_space(10.0);
        widgets::section(ui, Icon::Play, "Playing and converting");
        ui.add_space(8.0);
        setting(ui, "ffmpeg", |ui| match crate::audio::ffmpeg() {
            Some(p) => {
                if crate::audio::ffmpeg_is_bundled() {
                    ui.label("included with the app");
                    help(ui, "LGPL v2.1; licence and source details are in the app's Resources folder.");
                } else {
                    ui.label(format!("found at {}", p.display()));
                }
                help(ui, "Plays what the built-in decoder can't: Dolby E-AC-3/AC-3 (e.g. Atmos releases), TrueHD, DTS, Opus, APE, WavPack, WMA.");
                if crate::audio::ffmpeg_encodes("libmp3lame") {
                    help(ui, "Also converts songs for the card: lossless ones to FLAC, the rest to MP3 320.");
                } else {
                    ui.label(
                        RichText::new("This ffmpeg can't make MP3s (no LAME encoder): lossless songs are still converted to FLAC for the card, but AAC, Dolby and other lossy ones are left out.")
                            .size(13.0)
                            .color(ink(ui).warn),
                    );
                }
            }
            None => {
                ui.label(RichText::new("not found").color(ink(ui).warn));
                help(ui, "Install it (brew install ffmpeg) to play Dolby, Opus, APE, WavPack and WMA files here, and to convert songs the card's player can't play when writing to it.");
            }
        });

        ui.add_space(10.0);
        widgets::section(ui, Icon::Search, "MusicBrainz");
        ui.add_space(8.0);
        setting(ui, "Contact", |ui| {
            ui.add(egui::TextEdit::singleline(&mut app.saved.mb_contact).hint_text("email or URL (optional)").desired_width(320.0));
            help(ui, "MusicBrainz asks apps to say who's asking; this is sent with each lookup.");
        });
    });
}
