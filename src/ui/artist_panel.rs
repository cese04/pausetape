//! Under an artist opened in the library: names that look like theirs, who they are and where
//! to buy, hear or find out more of their music (from MusicBrainz), and renaming them.

use egui::{RichText, Ui};

use super::actions::Action;
use super::icons::Icon;
use super::text::plural;
use super::theme::ink;
use super::widgets::{chip, icon_link, muted, tag};
use crate::app::{App, ArtistState};
use crate::musicbrainz::{ArtistInfo, LinkUse};
use super::layout::{S1, S2, row, row_wrapped};

/// Artists whose names look like `cluster`'s, each with a decision to make. Shown whether the
/// artist is open or not: this is where the question comes up.
pub(super) fn look_alikes(app: &App, ui: &mut Ui, cluster: usize, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    for s in app.index.suggestions.iter().filter(|s| s.a == cluster || s.b == cluster) {
        let other = &app.index.clusters[if s.a == cluster { s.b } else { s.a }];
        let songs: usize = other.variants.values().sum();
        row(ui, |ui| {
            ui.add_space(S1);
            ui.label(RichText::new("≈").color(ink.warn));
            muted(ui, format!("looks like “{}” ({})", other.name, plural(songs, "song")));
            if icon_link(ui, Icon::Check, "Same artist").on_hover_text("One artist from now on, under the name most songs use.").clicked() {
                actions.push(Action::Merge(s.a, s.b));
            }
            if icon_link(ui, Icon::Close, "Different artists").on_hover_text("Not asked again. Settings › Artist decisions can undo it.").clicked() {
                actions.push(Action::Reject(s.a, s.b));
            }
        });
    }
}

/// The open artist's panel: facts, links and renaming.
pub(super) fn show(app: &App, ui: &mut Ui, cluster: usize, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let c = &app.index.clusters[cluster];
    let state = app.artist_info.get(&c.key);
    if state.is_none() {
        // Opened in an earlier session: what's stored, without asking anyone.
        actions.push(Action::ArtistFromDb(cluster));
    }
    let info = match state {
        Some(ArtistState::Found { info, .. }) => Some(info),
        _ => None,
    };
    // A card of its own, so it reads apart from the albums under it.
    let frame = egui::Frame::new()
        .fill(ink.hover)
        .stroke(egui::Stroke::new(1.0, ink.faint))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(S2 as i8))
        .outer_margin(egui::Margin { left: 0, right: 0, top: S1 as i8, bottom: S1 as i8 });
    frame.show(ui, |ui| {
        match state {
            Some(ArtistState::Found { info, .. }) => {
                let facts = facts(info);
                if !info.genres.is_empty() || !facts.is_empty() {
                    row_wrapped(ui, |ui| {
                        for g in info.genres.iter().take(5) {
                            tag(ui, g);
                        }
                        if !facts.is_empty() {
                            ui.add_space(S1);
                            ui.label(RichText::new(facts).size(13.5).color(ink.muted));
                        }
                    });
                }
            }
            Some(ArtistState::Asking) => {
                row(ui, |ui| {
                    super::cassette::spinner(ui, 12.0);
                    muted(ui, "asking MusicBrainz…");
                });
            }
            Some(ArtistState::Failed(e)) => {
                row_wrapped(ui, |ui| {
                    muted(ui, format!("MusicBrainz: {e}"));
                    if icon_link(ui, Icon::Rescan, "Try again").clicked() {
                        actions.push(Action::AskArtistInfo(cluster, None));
                    }
                });
            }
            Some(ArtistState::Choose(hits)) => {
                muted(ui, if hits.is_empty() { "MusicBrainz has no artist by this name." } else { "Which of these MusicBrainz artists is it?" });
                for h in hits {
                    row(ui, |ui| {
                        if icon_link(ui, Icon::Check, "This one").clicked() {
                            actions.push(Action::AskArtistInfo(cluster, Some(h.id.clone())));
                        }
                        ui.label(&h.name);
                        muted(ui, &h.detail);
                    });
                }
            }
            Some(ArtistState::Unasked) | None => {}
        }
        // Where to go for more: what MusicBrainz knows, and a search where it knows nothing.
        let links = info.map(|i| i.links.as_slice()).unwrap_or_default();
        for (kind, icon, label) in [(LinkUse::Buy, Icon::Buy, "buy"), (LinkUse::Listen, Icon::Listen, "listen"), (LinkUse::Discover, Icon::Discover, "discover")] {
            // (site, url, a search rather than the artist's own page)
            let mut row: Vec<(String, String, bool)> = links.iter().filter(|l| l.kind == kind).map(|l| (l.site.clone(), l.url.clone(), false)).collect();
            if kind == LinkUse::Buy {
                for (site, url) in searches(&c.name) {
                    if !row.iter().any(|(s, ..)| *s == site) {
                        row.push((site, url, true));
                    }
                }
            }
            if row.is_empty() {
                continue;
            }
            row_wrapped(ui, |ui| {
                ui.allocate_ui_with_layout(egui::vec2(96.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_width(96.0);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), egui::Sense::hover());
                    super::icons::paint(ui.painter(), rect, icon, ink.muted);
                    ui.label(RichText::new(label.to_uppercase()).size(11.0).extra_letter_spacing(1.2).color(ink.muted));
                });
                for (site, url, search) in row {
                    // Buying is what the panel is for: those links carry the accent.
                    let (icon, colour, hint) = match (search, kind) {
                        (true, _) => (Icon::Search, ink.muted, format!("Search {site} for “{}”", c.name)),
                        (false, LinkUse::Buy) => (Icon::External, ink.accent, url.clone()),
                        (false, _) => (Icon::External, ink.text, url.clone()),
                    };
                    if chip(ui, icon, &site, colour).on_hover_text(hint).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
                    }
                }
            });
        }
        row_wrapped(ui, |ui| {
            if let Some((rc, buf)) = &app.renaming
                && *rc == cluster
            {
                let mut text = buf.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut text).align(egui::Align2::LEFT_CENTER));
                if text != *buf {
                    actions.push(Action::Renaming(cluster, text.clone()));
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter || icon_link(ui, Icon::Check, "Rename in the tags").clicked() {
                    actions.push(Action::Rename(cluster, text));
                }
                if icon_link(ui, Icon::Close, "Cancel").clicked() {
                    actions.push(Action::StopRenaming);
                }
                return;
            }
            let hint = "Changes the artist and album artist of its songs, as corrections: written into the files when you write corrected tags, and into card copies.";
            if icon_link(ui, Icon::Rename, "Rename").on_hover_text(hint).clicked() {
                actions.push(Action::Renaming(cluster, c.name.clone()));
            }
            if let Some(i) = info
                && !i.name.is_empty()
                && i.name != c.name
                && icon_link(ui, Icon::Check, format!("Use MusicBrainz's name “{}”", i.name)).on_hover_text(hint).clicked()
            {
                actions.push(Action::Rename(cluster, i.name.clone()));
            }
            match state {
                Some(ArtistState::Unasked) => {
                    if icon_link(ui, Icon::Search, "Look up on MusicBrainz").on_hover_text("Who they are, and where to buy or hear more of their music.").clicked() {
                        actions.push(Action::AskArtistInfo(cluster, None));
                    }
                }
                Some(ArtistState::Found { info, fetched }) => {
                    if icon_link(ui, Icon::Search, "Not this artist?").on_hover_text("Choose another MusicBrainz artist by this name.").clicked() {
                        actions.push(Action::ChooseArtist(cluster));
                    }
                    let since = super::text::ago(*fetched);
                    if icon_link(ui, Icon::Rescan, "Refresh").on_hover_text(format!("Asked {since}.")).clicked() {
                        actions.push(Action::AskArtistInfo(cluster, Some(info.mbid.clone())));
                    }
                }
                _ => {}
            }
        });
    });
}

/// "US · since 1983", after the genres.
fn facts(i: &ArtistInfo) -> String {
    let years = match (i.begin, i.end) {
        (Some(b), Some(e)) => format!("{b}–{e}"),
        (Some(b), None) if !i.ended => format!("since {b}"),
        (Some(b), None) => format!("from {b}"),
        (None, Some(e)) => format!("until {e}"),
        (None, None) => String::new(),
    };
    [i.disambiguation.clone(), i.country.clone(), years].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// Searches for the stores in the buy row, for an artist MusicBrainz has no link to there.
fn searches(name: &str) -> [(String, String); 3] {
    let q = escape(name);
    [
        ("Bandcamp".into(), format!("https://bandcamp.com/search?q={q}&item_type=b")),
        ("Qobuz".into(), format!("https://www.qobuz.com/us-en/search/artists/{q}")),
        ("Discogs".into(), format!("https://www.discogs.com/search/?q={q}&type=artist")),
    ]
}

/// Percent-encode everything but unreserved characters, for a URL's path or query.
fn escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_escape_the_name() {
        assert_eq!(searches("Sigur Rós & Co")[0].1, "https://bandcamp.com/search?q=Sigur%20R%C3%B3s%20%26%20Co&item_type=b");
    }

    #[test]
    fn facts_in_a_line() {
        let i = ArtistInfo { genres: vec!["a".into(), "b".into()], country: "US".into(), begin: Some(1983), ..Default::default() };
        assert_eq!(facts(&i), "US · since 1983", "the genres are shown apart");
        let ended = ArtistInfo { begin: Some(1976), end: Some(1980), ended: true, ..Default::default() };
        assert_eq!(facts(&ended), "1976–1980");
    }
}
