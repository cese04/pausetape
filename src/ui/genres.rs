//! The genres view: every genre in the library, with how many albums and artists; a click shows
//! its artists and their albums, as the library by artist does. Albums without a genre of their
//! own count under their artist's (said so), and those with none at all come last.

use egui::{Align2, FontId, RichText, ScrollArea, Sense, Ui, vec2};

use super::layout::{ROW, S1, S2, row, font};
use super::text::plural;
use super::theme::ink;
use super::widgets;
use crate::app::{App, Tab};

/// The search term for a genre: its name in lower case, words joined by "-" ("genre:trip-hop").
pub fn term(name: &str) -> String {
    format!("genre:{}", name.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-"))
}

/// Show a genre's artists and albums (or, `None`, the albums without one).
pub fn open(app: &mut App, name: Option<&str>) {
    app.tab = Tab::Library;
    app.saved.album_table = false;
    (app.duplicates_only, app.look_alikes_only) = (false, false);
    app.filter = match name {
        Some(n) => term(n),
        None => "genre:none".into(),
    };
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let mut go: Option<Option<String>> = None;
    let by_size = app.genres_by_size;
    let index = app.genre_index();
    let mut rows: Vec<(String, usize, usize, usize)> = index
        .genres
        .iter()
        .map(|g| {
            let mut artists: Vec<usize> = g.albums.iter().map(|&(a, ..)| a).collect();
            artists.dedup();
            (g.name.clone(), g.albums.len(), artists.len(), g.albums.iter().filter(|x| x.2).count())
        })
        .collect();
    let none = index.none.len();
    if by_size {
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    }
    let count = rows.len();
    row(ui, |ui| {
        ui.label(RichText::new("Genres").size(font::HEADING).color(ink.text));
        widgets::muted(ui, plural(count, "genre"));
        ui.add_space(S2);
        if widgets::tab(ui, "By name", !by_size).clicked() {
            app.genres_by_size = false;
        }
        if widgets::tab(ui, "Most albums", by_size).clicked() {
            app.genres_by_size = true;
        }
    });
    widgets::help(ui, "From the songs' tags; an album without any takes its artist's genres from MusicBrainz, where they're known. An album with several genres is under each.");
    ui.add_space(S1);
    if rows.is_empty() && none == 0 {
        widgets::muted(ui, "No genres yet: the songs' tags name none. Look an album up on MusicBrainz to have its genres suggested, or open an artist to learn theirs.");
        return;
    }
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (name, albums, artists, from_artist) in rows {
            let mut facts = format!("{} · {}", plural(albums, "album"), plural(artists, "artist"));
            if from_artist > 0 {
                facts.push_str(&format!(" · {from_artist} by their artist's genre"));
            }
            if line(ui, &name, &facts, false).clicked() {
                go = Some(Some(name));
            }
        }
        if none > 0 && line(ui, "No genre", &plural(none, "album"), true).clicked() {
            go = Some(None);
        }
    });
    if let Some(g) = go {
        open(app, g.as_deref());
    }
}

/// One genre: its name, then what it holds, the whole row clickable.
fn line(ui: &mut Ui, name: &str, facts: &str, quiet: bool) -> egui::Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW + S1), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, name));
    widgets::focus_ring(ui, &resp, rect, 4.0);
    widgets::row_background(ui, &resp, rect, false);
    let y = rect.center().y;
    let p = ui.painter();
    let name_g = widgets::truncated(ui, name.to_string(), FontId::proportional(font::BODY), if quiet { ink.muted } else { ink.text }, rect.width() * 0.5);
    let name_w = name_g.size().x;
    p.galley(egui::pos2(rect.left() + S1, y - name_g.size().y / 2.0), name_g, ink.text);
    p.text(egui::pos2(rect.left() + S1 + name_w + S2, y), Align2::LEFT_CENTER, facts, FontId::proportional(font::UI), ink.muted);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
