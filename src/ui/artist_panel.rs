//! Under an artist opened in the library: names that look like theirs, who they are and where
//! to buy, hear or find out more of their music (from MusicBrainz), and renaming them.

use egui::{RichText, Ui};

use super::actions::Action;
use super::icons::Icon;
use super::text::plural;
use super::theme::ink;
use super::widgets::{icon_link, link, muted};
use crate::app::{App, ArtistState};
use crate::musicbrainz::{ArtistInfo, LinkUse};

/// Artists whose names look like `cluster`'s, each with a decision to make. Shown whether the
/// artist is open or not: this is where the question comes up.
pub(super) fn look_alikes(app: &App, ui: &mut Ui, cluster: usize, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    for s in app.index.suggestions.iter().filter(|s| s.a == cluster || s.b == cluster) {
        let other = &app.index.clusters[if s.a == cluster { s.b } else { s.a }];
        let songs: usize = other.variants.values().sum();
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("≈").color(ink.warn));
            muted(ui, format!("looks like “{}” ({})", other.name, plural(songs, "song")));
            if icon_link(ui, Icon::Check, "same artist").on_hover_text("One artist from now on, under the name most songs use.").clicked() {
                actions.push(Action::Merge(s.a, s.b));
            }
            if icon_link(ui, Icon::Close, "different artists").on_hover_text("Not asked again. Settings › Artist decisions can undo it.").clicked() {
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
    ui.indent(("artist panel", cluster), |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        match state {
            Some(ArtistState::Found { info, .. }) => {
                let facts = facts(info);
                if !facts.is_empty() {
                    ui.label(RichText::new(facts).size(13.5).color(ink.muted));
                }
            }
            Some(ArtistState::Asking) => {
                muted(ui, "asking MusicBrainz…");
            }
            Some(ArtistState::Failed(e)) => {
                ui.horizontal_wrapped(|ui| {
                    muted(ui, format!("MusicBrainz: {e}"));
                    if link(ui, "try again").clicked() {
                        actions.push(Action::AskArtistInfo(cluster, None));
                    }
                });
            }
            Some(ArtistState::Choose(hits)) => {
                muted(ui, if hits.is_empty() { "MusicBrainz has no artist by this name." } else { "Which of these MusicBrainz artists is it?" });
                for h in hits {
                    ui.horizontal(|ui| {
                        ui.label(&h.name);
                        muted(ui, &h.detail);
                        if link(ui, "this one").clicked() {
                            actions.push(Action::AskArtistInfo(cluster, Some(h.id.clone())));
                        }
                    });
                }
            }
            Some(ArtistState::Unasked) | None => {}
        }
        // Where to go for more: what MusicBrainz knows, and a search where it knows nothing.
        let links = info.map(|i| i.links.as_slice()).unwrap_or_default();
        for (kind, label) in [(LinkUse::Buy, "buy"), (LinkUse::Listen, "listen"), (LinkUse::Discover, "discover")] {
            let mut row: Vec<(String, String)> = links.iter().filter(|l| l.kind == kind).map(|l| (l.site.clone(), l.url.clone())).collect();
            if kind == LinkUse::Buy {
                for (site, url) in searches(&c.name) {
                    if !row.iter().any(|(s, _)| *s == site) {
                        row.push((format!("search {site}"), url));
                    }
                }
            }
            if row.is_empty() {
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(62.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_width(62.0);
                    ui.label(RichText::new(label).size(12.5).color(ink.muted));
                });
                for (site, url) in row {
                    if link(ui, &site).on_hover_text(&url).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
                    }
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if let Some((rc, buf)) = &app.renaming
                && *rc == cluster
            {
                let mut text = buf.clone();
                let r = ui.text_edit_singleline(&mut text);
                if text != *buf {
                    actions.push(Action::Renaming(cluster, text.clone()));
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter || icon_link(ui, Icon::Check, "rename in the tags").clicked() {
                    actions.push(Action::Rename(cluster, text));
                }
                if icon_link(ui, Icon::Close, "cancel").clicked() {
                    actions.push(Action::StopRenaming);
                }
                return;
            }
            let hint = "Changes the artist and album artist of its songs, as corrections: written into the files when you write corrected tags, and into card copies.";
            if icon_link(ui, Icon::Rename, "rename").on_hover_text(hint).clicked() {
                actions.push(Action::Renaming(cluster, c.name.clone()));
            }
            if let Some(i) = info
                && !i.name.is_empty()
                && i.name != c.name
                && icon_link(ui, Icon::Check, format!("use MusicBrainz's name “{}”", i.name)).on_hover_text(hint).clicked()
            {
                actions.push(Action::Rename(cluster, i.name.clone()));
            }
            match state {
                Some(ArtistState::Unasked) => {
                    if icon_link(ui, Icon::Search, "look up on MusicBrainz").on_hover_text("Who they are, and where to buy or hear more of their music.").clicked() {
                        actions.push(Action::AskArtistInfo(cluster, None));
                    }
                }
                Some(ArtistState::Found { info, fetched }) => {
                    if icon_link(ui, Icon::Search, "not this artist?").on_hover_text("Choose another MusicBrainz artist by this name.").clicked() {
                        actions.push(Action::ChooseArtist(cluster));
                    }
                    let since = super::text::ago(*fetched);
                    if icon_link(ui, Icon::Rescan, "refresh").on_hover_text(format!("Asked {since}.")).clicked() {
                        actions.push(Action::AskArtistInfo(cluster, Some(info.mbid.clone())));
                    }
                }
                _ => {}
            }
        });
    });
}

/// "experimental hip hop · downtempo · US · since 1983"
fn facts(i: &ArtistInfo) -> String {
    let years = match (i.begin, i.end) {
        (Some(b), Some(e)) => format!("{b}–{e}"),
        (Some(b), None) if !i.ended => format!("since {b}"),
        (Some(b), None) => format!("from {b}"),
        (None, Some(e)) => format!("until {e}"),
        (None, None) => String::new(),
    };
    let genres = i.genres.iter().take(4).cloned();
    [i.disambiguation.clone()].into_iter().chain(genres).chain([i.country.clone(), years]).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
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
        assert_eq!(facts(&i), "a · b · US · since 1983");
        let ended = ArtistInfo { begin: Some(1976), end: Some(1980), ended: true, ..Default::default() };
        assert_eq!(facts(&ended), "1976–1980");
    }
}
