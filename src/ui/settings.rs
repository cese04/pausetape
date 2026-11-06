//! The settings tab.

use egui::{RichText, Ui};

use super::icons::Icon;
use super::theme::{Accent, ink};
use super::widgets::{self, help, icon_link, setting};
use crate::app::{App, ThemePref};
use super::layout::{S1, S2, row};

pub fn show(app: &mut App, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        widgets::section(ui, Icon::Folder, "Music folder");
        ui.add_space(S1);
        super::folders::settings_section(app, ui);
        ui.add_space(S2);
        widgets::section(ui, Icon::Settings, "Appearance");
        ui.add_space(S1);
        setting(ui, "Colours", |ui| {
            row(ui, |ui| {
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
            row(ui, |ui| {
                let dark = ui.visuals().dark_mode;
                for accent in Accent::ALL {
                    if widgets::swatch(ui, accent.colour(dark), app.saved.accent == accent, accent.name()).on_hover_text(accent.name()).clicked() {
                        app.saved.accent = accent;
                        app.apply_theme();
                    }
                }
                ui.add_space(S1);
                ui.label(RichText::new(app.saved.accent.name()).color(ink(ui).muted));
            });
            help(ui, "Marks the open tab, the main button, progress and what's playing. Warnings stay red.");
        });

        ui.add_space(S2);
        widgets::section(ui, Icon::Library, "Library");
        ui.add_space(S1);
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
            row(ui, |ui| {
                if app.busy.is_none() && icon_link(ui, Icon::Rescan, "Read every file again").on_hover_text("for tags edited by tools that don't update the file's date").clicked() {
                    app.read_all_again();
                }
                if icon_link(ui, Icon::Trash, "Forget stored tags").on_hover_text("asks first; unsaved corrections are kept").clicked() {
                    app.confirm = Some(crate::app::Confirm::ForgetTags);
                }
            });
        });
        setting(ui, "Artist decisions", |ui| {
            let p = &app.saved.prefs;
            if p.merges.is_empty() && p.rejected.is_empty() && p.names.is_empty() {
                ui.label("none yet");
            }
            // An artist key as the tags spell it, or as the key itself once no song does.
            let name = |key: &str| {
                let spelt = app.index.clusters.iter().flat_map(|c| c.variants.keys()).find(|v| crate::library::names::artist_key(v) == key);
                spelt.cloned().unwrap_or_else(|| key.to_string())
            };
            let mut undo = None;
            for (i, (a, b)) in p.merges.iter().enumerate() {
                row(ui, |ui| {
                    ui.label(format!("“{}” and “{}”: the same artist", name(a), name(b)));
                    if icon_link(ui, Icon::Close, "Undo").clicked() {
                        undo = Some(Undo::Merge(i));
                    }
                });
            }
            for (a, b) in &p.rejected {
                row(ui, |ui| {
                    ui.label(format!("“{}” and “{}”: different artists", name(a), name(b)));
                    if icon_link(ui, Icon::Close, "Undo").clicked() {
                        undo = Some(Undo::Reject(a.clone(), b.clone()));
                    }
                });
            }
            for (k, n) in &p.names {
                row(ui, |ui| {
                    ui.label(format!("{k} shown as “{n}”"));
                    if icon_link(ui, Icon::Close, "Undo").clicked() {
                        undo = Some(Undo::Name(k.clone()));
                    }
                });
            }
            help(ui, "What you said in the library about artists whose names look alike. Renaming an artist changes its songs' tags instead, like any correction.");
            if let Some(u) = undo {
                let p = &mut app.saved.prefs;
                match u {
                    Undo::Merge(i) => {
                        p.merges.remove(i);
                    }
                    Undo::Reject(a, b) => {
                        p.rejected.remove(&(a, b));
                    }
                    Undo::Name(k) => {
                        p.names.remove(&k);
                    }
                }
                app.rebuild();
            }
        });

        ui.add_space(S2);
        widgets::section(ui, Icon::Play, "Playing and converting");
        ui.add_space(S1);
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
                help(
                    ui,
                    format!(
                        "Install it ({}) to play Dolby, Opus, APE, WavPack and WMA files here, and to convert songs the card's player can't play when writing to it.",
                        crate::platform::FFMPEG_INSTALL
                    ),
                );
            }
        });

        ui.add_space(S2);
        widgets::section(ui, Icon::Info, "Keyboard");
        ui.add_space(S1);
        // Keys in the column where settings have their names.
        for (keys, what) in super::keys::list(ui.ctx()) {
            row(ui, |ui| {
                ui.allocate_ui_with_layout(egui::vec2(widgets::LABEL_W, 0.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_width(widgets::LABEL_W);
                    ui.label(RichText::new(keys).color(ink(ui).text));
                });
                ui.label(RichText::new(what).color(ink(ui).muted));
            });
        }
        help(ui, "The arrows, Enter and the shortcuts for selecting, tags and the card act on the library. Shortcuts wait while you type in a field or a dialog is open.");

        ui.add_space(S2);
        widgets::section(ui, Icon::Search, "MusicBrainz");
        ui.add_space(S1);
        setting(ui, "Contact", |ui| {
            ui.add(egui::TextEdit::singleline(&mut app.saved.mb_contact).align(egui::Align2::LEFT_CENTER).hint_text("email or URL (optional)").desired_width(320.0));
            help(ui, "MusicBrainz asks apps to say who's asking; this is sent with each lookup.");
        });
    });
}

/// One artist decision to take back.
enum Undo {
    Merge(usize),
    Reject(String, String),
    Name(String),
}
