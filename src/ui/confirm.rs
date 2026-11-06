//! Asking before a change that can't be undone: what it does, what it costs, and a way back.

use egui::{RichText, Ui};

use super::icons::Icon;
use super::layout::{S1, S2, row};
use super::text::plural;
use super::theme::ink;
use super::widgets;
use crate::app::{App, Confirm};

/// The open question, if any, over whatever tab is open.
pub fn dialog(app: &mut App, ui: &mut Ui) {
    let Some(asking) = app.confirm.clone() else { return };
    let (title, body, yes, icon) = match &asking {
        Confirm::DeleteWrite(id) => {
            let Some(w) = app.saved.writes.iter().find(|w| &w.id == id) else {
                app.confirm = None;
                return;
            };
            let files = w.manifest.len();
            let body = if files == 0 {
                "Pausetape forgets this card setup: the music chosen for it, its names and order on the card. Nothing on the card changes.".to_string()
            } else {
                format!(
                    "Pausetape forgets this card setup: the music chosen for it, its names and order, and its record of the {} it put on the card. Those files stay on the card, but no setup will know they're its own, so none will update them or take them off. To clear them from the card first, untick everything and Sync, then delete the setup.",
                    plural(files, "file")
                )
            };
            (format!("Delete the card setup “{}”?", w.name), body, "Delete it", Icon::Trash)
        }
        Confirm::ForgetTags => {
            let songs = app.db.as_ref().map(|db| db.counts().0).unwrap_or(0);
            let body = format!(
                "Pausetape forgets the tags it keeps of {}. Nothing in your music files changes, and corrections you haven't written to them stay. The next time the folder is read, every file is opened again, which takes a while on a large library or a slow drive.",
                plural(songs, "song")
            );
            ("Forget the stored tags?".to_string(), body, "Forget them", Icon::Trash)
        }
    };
    let ink = ink(ui);
    let mut answer = None;
    let r = egui::Modal::new(egui::Id::unique("confirm")).frame(super::theme::dialog_frame(ui)).show(ui.ctx(), |ui| {
        ui.set_width(520.0f32.min(ui.ctx().content_rect().width() - 80.0));
        ui.label(RichText::new(title).size(18.0).color(ink.text));
        ui.add_space(S1);
        widgets::help(ui, body);
        ui.add_space(S2);
        row(ui, |ui| {
            if widgets::primary_with(ui, icon, yes).clicked() {
                answer = Some(true);
            }
            if widgets::button(ui, "Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    if r.should_close() {
        answer.get_or_insert(false);
    }
    match answer {
        Some(true) => {
            app.confirm = None;
            match asking {
                Confirm::DeleteWrite(id) => app.delete_write(&id),
                Confirm::ForgetTags => {
                    if let Some(db) = &mut app.db {
                        let _ = db.clear();
                    }
                    app.notice("forgot the stored tags · the next rescan reads every file");
                }
            }
        }
        Some(false) => app.confirm = None,
        None => {}
    }
}
