//! The artists tab: possible duplicates to merge or keep apart, names, MusicBrainz matches.

use egui::{Align, Layout, RichText, ScrollArea, Ui};

use super::MAX_W;
use super::actions::{Action, apply};
use super::theme::ink;
use super::icons::Icon;
use super::widgets::{caps, icon_link, muted};
use crate::app::App;

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let mut actions = Vec::new();
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.set_max_width(ui.available_width().min(MAX_W));
        caps(ui, "possible duplicates");
        if app.index.suggestions.is_empty() {
            muted(ui, "None. Spelling variants like “The Beatles” / “Beatles, The” or “AC/DC” / “ACDC” are already merged.");
        }
        for s in &app.index.suggestions {
            let (a, b) = (&app.index.clusters[s.a], &app.index.clusters[s.b]);
            let count = |c: &crate::library::grouping::Cluster| c.variants.values().sum::<usize>();
            ui.horizontal(|ui| {
                ui.label(&a.name);
                muted(ui, format!("({})", count(a)));
                ui.label(RichText::new("≈").color(ink.warn));
                ui.label(&b.name);
                muted(ui, format!("({})", count(b)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_link(ui, Icon::Close, "different artists").clicked() {
                        actions.push(Action::Reject(s.a, s.b));
                    }
                    if icon_link(ui, Icon::Check, "same artist").clicked() {
                        actions.push(Action::Merge(s.a, s.b));
                    }
                });
            });
        }

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            caps(ui, "all artists");
            muted(ui, "the first name is the folder name; greyed ones are other spellings in your tags.");
        });
        if icon_link(ui, Icon::Search, format!("ask MusicBrainz about all {} artists (~1 per second)", app.index.clusters.len())).clicked() {
            for c in 0..app.index.clusters.len() {
                if !app.artist_hits.contains_key(&app.index.clusters[c].key) {
                    actions.push(Action::LookupArtist(c));
                }
            }
        }
        ui.add_space(6.0);
        for node in &app.index.artists {
            let ci = node.cluster;
            let c = &app.index.clusters[ci];
            ui.horizontal(|ui| {
                if let Some((rc, buf)) = &mut app.renaming
                    && *rc == ci
                {
                    let r = ui.text_edit_singleline(buf);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || icon_link(ui, Icon::Check, "save").clicked() {
                        actions.push(Action::Name(ci, buf.clone()));
                    }
                } else {
                    ui.label(RichText::new(&c.name).size(16.0));
                    let others: Vec<String> = c.variants.iter().filter(|(v, _)| **v != c.name).map(|(v, n)| format!("{v} ×{n}")).collect();
                    if !others.is_empty() {
                        muted(ui, others.join(" · "));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_link(ui, Icon::Search, "musicbrainz").clicked() {
                        actions.push(Action::LookupArtist(ci));
                    }
                    if icon_link(ui, Icon::Rename, "rename").clicked() {
                        app.renaming = Some((ci, c.name.clone()));
                    }
                });
            });
            match app.artist_hits.get(&c.key) {
                Some(None) => {
                    ui.indent(ci, |ui| muted(ui, "asking MusicBrainz…"));
                }
                Some(Some(Err(e))) => {
                    ui.indent(ci, |ui| muted(ui, e.as_str()));
                }
                Some(Some(Ok(hits))) => {
                    ui.indent(ci, |ui| {
                        if hits.is_empty() {
                            muted(ui, "no match");
                        }
                        // Exact name match already: nothing to suggest.
                        if hits.first().is_some_and(|h| h.name == c.name && h.score >= 95) {
                            muted(ui, "✓ matches MusicBrainz");
                            return;
                        }
                        for h in hits.iter().take(3) {
                            ui.horizontal(|ui| {
                                ui.label(&h.name);
                                muted(ui, format!("{} · score {}", h.detail, h.score));
                                if icon_link(ui, Icon::Check, "use this name").clicked() {
                                    actions.push(Action::Name(ci, h.name.clone()));
                                }
                            });
                        }
                    });
                }
                None => {}
            }
        }
    });
    apply(app, actions);
}

