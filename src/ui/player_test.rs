//! Testing the card's player: write a folder of samples to the card, try them on the player,
//! and tick what worked. What's ticked corrects the player's profile for this card setup.

use egui::{RichText, ScrollArea, Sense, Ui, vec2};

use super::icons::Icon;
use super::text::plural;
use super::theme::ink;
use super::widgets::{self, muted};
use crate::app::App;
use crate::card::Sample;
use crate::card::testcard::{TEST_FOLDER, file_name};

pub fn dialog(app: &mut App, ui: &mut Ui) {
    if !app.player_test_open {
        return;
    }
    let ink = ink(ui);
    let frame = egui::Frame::popup(ui.style()).fill(ink.bg).inner_margin(egui::Margin::same(22));
    let r = egui::Modal::new(egui::Id::unique("player test")).frame(frame).show(ui.ctx(), |ui| {
        ui.set_width(700.0f32.min(ui.ctx().content_rect().width() - 80.0));
        let player = app.player();
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            super::icons::paint(ui.painter(), rect, Icon::Card, ink.text);
            ui.label(RichText::new(format!("Test the player: {}", player.name())).size(18.0).color(ink.text));
        });
        ui.add_space(6.0);
        muted(
            ui,
            format!(
                "Writes a folder, “{TEST_FOLDER}”, of 6-second tones in every format, a cover and accented titles, and a playlist. Eject the card, try each on the player, and tick what worked. Your answers are used instead of what's known of this player from its makers and reviews, for this card setup."
            ),
        );
        ui.add_space(8.0);
        if let Some(b) = &app.busy {
            ui.horizontal(|ui| {
                super::cassette::spinner(ui, 14.0);
                muted(ui, format!("{}…", b.label));
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                let on_card = app.test_card_on_card();
                if app.write().dest.is_none() {
                    ui.label(RichText::new("Choose the card first (destination, in the card tab).").color(ink.warn));
                } else if widgets::primary_with(ui, Icon::Card, if on_card { "Write the test folder again" } else { "Write the test folder to the card" }).clicked() {
                    app.write_test_card();
                }
                if on_card && widgets::icon_link(ui, Icon::Remove, "take it off the card").on_hover_text("removes only the files the test wrote").clicked() {
                    app.remove_test_card();
                }
            });
            if crate::audio::ffmpeg().is_none() {
                ui.label(RichText::new("The samples are made with ffmpeg, which isn't installed (brew install ffmpeg).").size(13.0).color(ink.warn));
            }
        }
        ui.add_space(10.0);
        let answered = app.write().options.tested.get(&app.write().options.device).map_or(0, |r| r.len());
        ui.horizontal(|ui| {
            widgets::caps(ui, "what worked on the player");
            if answered > 0 {
                muted(ui, format!("{} answered", plural(answered, "sample")));
            }
        });
        ui.add_space(4.0);
        let made = app.test_card.clone();
        let mut set = None;
        ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.5).show(ui, |ui| {
            egui::Grid::new("player test grid").num_columns(4).spacing([14.0, 6.0]).striped(false).show(ui, |ui| {
                for s in Sample::ALL {
                    let label = file_name(s);
                    ui.label(RichText::new(s.label()).color(ink.text)).on_hover_text(if s == Sample::Playlist { label } else { format!("{TEST_FOLDER}/{label}") });
                    muted(ui, s.question());
                    let current = app.write().options.tested.get(&app.write().options.device).and_then(|r| r.get(&s)).copied();
                    ui.horizontal(|ui| {
                        for (answer, text) in [(Some(true), "yes"), (Some(false), "no"), (None, "not tried")] {
                            if ui.radio(current == answer, text).clicked() && current != answer {
                                set = Some((s, answer));
                            }
                        }
                    });
                    match made.as_ref().and_then(|m| m.iter().find(|(x, _)| *x == s)).map(|(_, r)| r) {
                        Some(Err(why)) => ui.label(RichText::new("not written").size(12.5).color(ink.warn)).on_hover_text(why),
                        _ => ui.label(""),
                    };
                    ui.end_row();
                }
            });
        });
        if let Some((s, answer)) = set {
            app.set_tested(s, answer);
        }
        ui.add_space(10.0);
        // What the answers change.
        let player = app.player();
        if answered > 0 {
            widgets::help(ui, player.plays_text());
            if let Some(rate) = player.max_rate() {
                widgets::help(ui, format!("Songs above {} kHz are written at CD quality.", rate / 1000));
            }
            widgets::help(ui, if player.shows_covers() { "Covers: shown, so missing ones are worth finding." } else { "Covers: not shown on this player." });
        }
        ui.add_space(6.0);
        if widgets::button(ui, "Done").clicked() {
            app.player_test_open = false;
        }
    });
    if r.should_close() {
        app.player_test_open = false;
    }
}
