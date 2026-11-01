//! Editing tags by hand: the dialog, and the bar that offers it for the songs chosen in the
//! library.

use egui::{RichText, Ui};

use super::actions::Action;
use super::icons::Icon;
use super::text::plural;
use super::theme::{self, ink};
use super::widgets::{self, help};
use crate::app::{App, Shared};
use super::layout::{S1, S2, row, row_wrapped};

const LABEL_W: f32 = 110.0;

/// Above the library list while songs are selected: what's chosen and what to do with it.
pub fn selection_bar(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    if app.tag_edit.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        actions.push(Action::ClearSelection);
    }
    theme::panel_frame(ui).inner_margin(egui::Margin::symmetric(S2 as i8, S1 as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        row_wrapped(ui, |ui| {
            ui.label(RichText::new(format!("{} selected", plural(app.selected.len(), "song"))).color(ink.text));
            ui.add_space(S1);
            let songs: Vec<usize> = app.selected.iter().copied().collect();
            if widgets::icon_link(ui, Icon::Rename, "edit their tags").clicked() {
                actions.push(Action::EditTags(songs.clone()));
            }
            if widgets::icon_link(ui, Icon::Play, "play them").clicked() {
                actions.push(Action::Play(songs, 0));
            }
            if widgets::icon_link(ui, Icon::Close, "clear").on_hover_text("or press Esc").clicked() {
                actions.push(Action::ClearSelection);
            }
            help(ui, "⌘-click adds or removes a song, shift-click a run of songs.");
        });
    });
    ui.add_space(S1);
}

/// The tag editor, over whatever tab is open.
pub fn dialog(app: &mut App, ui: &mut Ui) {
    let Some(edit) = &mut app.tag_edit else { return };
    let ink = ink(ui);
    let (mut save, mut cancel) = (false, false);
    let frame = super::theme::dialog_frame(ui);
    egui::Modal::new(egui::Id::unique("tag editor")).frame(frame).show(ui.ctx(), |ui| {
        ui.set_width(560.0f32.min(ui.ctx().content_rect().width() - 80.0));
        row(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            super::icons::paint(ui.painter(), rect, Icon::Rename, ink.text);
            ui.label(RichText::new("Edit tags").size(18.0).color(ink.text));
            ui.label(RichText::new(plural(edit.tracks.len(), "song")).color(ink.muted));
        });
        ui.add_space(S2);
        egui::Grid::new("shared tags").num_columns(2).min_col_width(LABEL_W).spacing([12.0, 8.0]).show(ui, |ui| {
            for (label, field, width) in [
                ("Album", &mut edit.album, 400.0),
                ("Album artist", &mut edit.album_artist, 400.0),
                ("Artist", &mut edit.artist, 400.0),
                ("Year", &mut edit.year, 70.0),
                ("Disc", &mut edit.disc, 70.0),
            ] {
                ui.label(RichText::new(label).color(ink.muted));
                shared_box(ui, field, width);
                ui.end_row();
            }
        });
        ui.add_space(S2);
        ui.label(RichText::new(if edit.tracks.len() == 1 { "Number and title" } else { "Numbers and titles" }).color(ink.muted));
        ui.add_space(S1);
        egui::ScrollArea::vertical().max_height(280.0).auto_shrink([false, true]).show(ui, |ui| {
            for (k, (number, title)) in edit.songs.iter_mut().enumerate() {
                let t = &app.tracks[edit.tracks[k]];
                row(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(number).desired_width(36.0).align(egui::Align2::RIGHT_CENTER));
                    let r = ui.add(egui::TextEdit::singleline(title).align(egui::Align2::LEFT_CENTER).hint_text(t.display_title()).desired_width(ui.available_width() - 8.0));
                    r.on_hover_text(t.id().display().to_string());
                });
            }
        });
        ui.add_space(S2);
        if let Some(p) = edit.problem() {
            ui.label(RichText::new(p).size(13.0).color(ink.warn));
        }
        let cue = edit.tracks.iter().any(|&i| app.tracks[i].cue.is_some());
        help(
            ui,
            format!(
                "An empty box leaves the songs as they are. Changes stay in the app and reach the card at the next Sync; the music files change only with \"write corrected tags\"{}.",
                if cue { " (songs inside an album file never do)" } else { "" }
            ),
        );
        ui.add_space(S2);
        row(ui, |ui| {
            let can = edit.problem().is_none();
            if ui.add_enabled_ui(can, |ui| widgets::primary_with(ui, Icon::Check, "Save")).inner.on_hover_text("or ⌘-Return").clicked() {
                save = true;
            }
            if widgets::button_with(ui, Icon::Close, "Cancel").on_hover_text("or Esc").clicked() {
                cancel = true;
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
            save = true;
        }
    });
    if save {
        app.save_tag_edit();
    } else if cancel {
        app.tag_edit = None;
    }
}

/// A field all the songs share; when they differ, the box starts empty and says so.
fn shared_box(ui: &mut Ui, f: &mut Shared, width: f32) {
    let hint = match (f.mixed(), width < 100.0) {
        (false, _) => "",
        (true, true) => "several",
        (true, false) => "several values · type one to set it for all",
    };
    ui.add(egui::TextEdit::singleline(&mut f.text).align(egui::Align2::LEFT_CENTER).desired_width(width).hint_text(hint));
}
