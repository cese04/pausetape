//! The details of what's selected in the library, as Calibre shows a book's beside its list: the
//! album's cover, what it is, where it stands with the card, what can be done with it, and its
//! songs. With nothing selected, what's playing. Beside the library where there's room for it,
//! else as a sheet over the page (right-click › Details).

use egui::{Align2, FontId, RichText, ScrollArea, Sense, Ui, vec2};

use super::actions::Action;
use super::icons::Icon;
use super::layout::{ROW, S1, S2, S3};
use super::library::{Mark, album_status};
use super::text::{self, plural};
use super::theme::ink;
use super::widgets;
use crate::app::App;

/// The panel's width, and the least the library keeps beside it.
pub const WIDTH: f32 = 300.0;
const LIBRARY_KEEPS: f32 = 700.0;

/// Room for the panel beside a library this wide.
pub fn fits(width: f32) -> bool {
    width >= WIDTH + LIBRARY_KEEPS
}

/// What the panel is about.
enum About {
    /// One album, and whether it's what's playing rather than what's selected.
    Album(usize, usize, bool),
    /// Several albums selected.
    Several(Vec<(usize, usize)>),
    Nothing,
}

fn about(app: &App) -> About {
    let albums: Vec<(usize, usize)> = if app.saved.album_table {
        app.selected_albums.iter().filter_map(|id| app.album_by_id(id)).collect()
    } else {
        app.albums_of(&app.selected)
    };
    match albums[..] {
        [(a, b)] => About::Album(a, b, false),
        [] => match app.current().map(|t| app.albums_of(&[t].into())).as_deref() {
            Some([(a, b), ..]) => About::Album(*a, *b, true),
            _ => About::Nothing,
        },
        _ => About::Several(albums),
    }
}

/// The panel's contents, in a column `WIDTH` wide.
pub fn show(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let width = ui.available_width();
    ScrollArea::vertical().id_salt("details").auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(width);
        ui.set_max_width(width);
        match about(app) {
            About::Album(a, b, playing) => album(app, ui, a, b, playing, actions),
            About::Several(albums) => several(app, ui, &albums, actions),
            About::Nothing => {
                ui.add_space(S3);
                ui.label(RichText::new("Select an album to see it here: its cover, its songs, and where it stands with the card.").color(ink.muted));
            }
        }
    });
}

/// The same, as a sheet over the page.
pub fn sheet(app: &mut App, ui: &mut Ui) {
    if !app.details_sheet {
        return;
    }
    let mut actions = Vec::new();
    let r = egui::Modal::new(egui::Id::unique("details sheet")).frame(super::theme::dialog_frame(ui)).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH + S3);
        ui.set_max_height(ui.ctx().content_rect().height() - 120.0);
        show(app, ui, &mut actions);
        ui.add_space(S1);
        if widgets::button(ui, "Done").clicked() {
            app.details_sheet = false;
        }
    });
    if r.should_close() {
        app.details_sheet = false;
    }
    super::actions::apply(app, actions);
}

fn album(app: &mut App, ui: &mut Ui, ai: usize, bi: usize, playing: bool, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let artist = &app.index.artists[ai];
    let al = &artist.albums[bi];
    let ck = app.index.clusters[artist.cluster].key.clone();
    let first = app.tracks[al.tracks[0]].path.clone();
    let tracks = al.tracks.clone();
    let title = al.title.clone().unwrap_or_else(|| "Loose tracks".into());
    let artist_name = artist.name.clone();
    if playing {
        widgets::caps(ui, "playing");
        ui.add_space(S1 / 2.0);
    }
    // The cover, square, the panel's width.
    app.want_detail_cover(&first);
    let side = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    match app.detail_cover.as_ref().and_then(|(_, t)| t.as_ref()) {
        Some(tex) => {
            let size = tex.size_vec2();
            let scale = (side / size.x).min(side / size.y);
            egui::Image::new(tex).corner_radius(3.0).paint_at(ui, egui::Rect::from_center_size(rect.center(), size * scale));
        }
        None => {
            ui.painter().rect_filled(rect, 3.0, ink.hover);
            let initial = title.chars().next().unwrap_or('♪');
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, initial, FontId::proportional(side * 0.3), ink.faint);
        }
    }
    ui.add_space(S2);
    ui.add(egui::Label::new(RichText::new(&title).italics().size(20.0).color(ink.text)).wrap());
    if widgets::link(ui, &artist_name).on_hover_text("this artist's albums, and more about them").clicked() {
        actions.push(Action::ShowArtist(ai));
    }
    let al = &app.index.artists[ai].albums[bi];
    let rows = app.album_rows(&crate::library::search::Query::default());
    let facts: Vec<String> = match rows.iter().find(|r| r.artist == ai && r.album == bi) {
        Some(r) => [r.year.map(|y| y.to_string()), Some(r.format.clone()), Some(plural(r.songs, "song")), Some(text::bytes(r.bytes))].into_iter().flatten().collect(),
        None => vec![plural(al.tracks.len(), "song")],
    };
    ui.label(RichText::new(facts.join(" · ")).size(13.5).color(ink.muted));
    let (mark, status) = album_status(app, &ck, &app.index.artists[ai], al);
    if let Some(m) = mark {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, ROW), Sense::hover());
            m.paint(ui.painter(), r.center(), 3.5, &ink);
            ui.label(RichText::new(&status).size(13.5).color(if m == Mark::Removing { ink.warn } else { ink.muted }));
        });
    }
    ui.add_space(S1);
    // What can be done with it: the menu's commands, laid out.
    let chosen = app.write().custom.includes(&ck, al, &app.index.artists[ai].albums);
    ui.horizontal_wrapped(|ui| {
        if widgets::icon_link(ui, Icon::Play, "Play").clicked() {
            actions.push(Action::Play(tracks.clone(), 0));
        }
        let (icon, words) = if chosen { (Icon::Remove, "Take off the card") } else { (Icon::Add, "Send to the card") };
        if widgets::icon_link(ui, icon, words).clicked() {
            actions.push(Action::CardAlbum(ai, bi, !chosen));
        }
        if widgets::icon_link(ui, Icon::Rename, "Edit tags").clicked() {
            actions.push(Action::EditTags(tracks.clone()));
        }
        if widgets::icon_link(ui, Icon::Search, "Look up").on_hover_text("suggest tags from MusicBrainz").clicked() {
            actions.push(Action::LookupAlbum(ai, bi));
        }
        if app.lacks_cover(al) && app.busy.is_none() && widgets::icon_link(ui, Icon::Image, "Find cover").clicked() {
            actions.push(Action::FindCovers(vec![(ai, bi)]));
        }
        if widgets::icon_link(ui, Icon::Folder, "Open folder").clicked()
            && let Some(dir) = first.parent()
        {
            crate::platform::reveal(dir);
        }
    });
    // Its MusicBrainz suggestions, when it's been looked up.
    let id = app.index.artists[ai].albums[bi].id(&ck);
    if let Some(l) = app.lookups.get_mut(&id) {
        ui.add_space(S1);
        super::library::lookup_block(ui, &app.tracks, &id, l, actions);
    }
    ui.add_space(S2);
    widgets::caps(ui, "songs");
    ui.add_space(S1 / 2.0);
    let current = app.current();
    for (pos, &t) in tracks.iter().enumerate() {
        let track = &app.tracks[t];
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        widgets::row_background(ui, &resp, rect, false);
        let now = current == Some(t);
        let color = if now { ink.accent } else { ink.text };
        let y = rect.center().y;
        let number = if now { "▸".to_string() } else { track.track.map(|n| n.to_string()).unwrap_or_default() };
        ui.painter().text(egui::pos2(rect.left() + S2 + S1, y), Align2::RIGHT_CENTER, number, FontId::proportional(13.0), if now { ink.accent } else { ink.muted });
        let dur = text::duration(track.duration);
        let dur_w = widgets::text_width(ui, &dur, 13.0);
        let g = widgets::truncated(ui, track.display_title(), FontId::proportional(14.0), color, rect.width() - S3 - S1 - dur_w - S2);
        ui.painter().galley(egui::pos2(rect.left() + S3 + S1, y - g.size().y / 2.0), g, color);
        ui.painter().text(egui::pos2(rect.right() - S1, y), Align2::RIGHT_CENTER, dur, FontId::proportional(13.0), ink.muted);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, track.display_title()));
        let resp = resp.on_hover_text("double-click to play from here");
        if resp.double_clicked() {
            actions.push(Action::Play(tracks.clone(), pos));
        }
        resp.context_menu(|ui| super::menu::music(app, ui, super::menu::Of::Songs(vec![t]), actions, |_, _| {}));
    }
}

fn several(app: &App, ui: &mut Ui, albums: &[(usize, usize)], actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let songs: Vec<usize> = albums.iter().flat_map(|&(a, b)| app.index.artists[a].albums[b].tracks.iter().copied()).collect();
    let bytes: u64 = songs.iter().map(|&t| app.tracks[t].size).sum();
    ui.add_space(S1);
    ui.label(RichText::new(format!("{} selected", plural(albums.len(), "album"))).size(20.0).color(ink.text));
    ui.label(RichText::new(format!("{} · {}", plural(songs.len(), "song"), text::bytes(bytes))).size(13.5).color(ink.muted));
    ui.add_space(S1);
    let on = app.all_on_card(albums);
    ui.horizontal_wrapped(|ui| {
        if widgets::icon_link(ui, Icon::Play, "Play").clicked() {
            actions.push(Action::Play(songs.clone(), 0));
        }
        let (icon, words) = if on { (Icon::Remove, "Take off the card") } else { (Icon::Add, "Send to the card") };
        if widgets::icon_link(ui, icon, words).clicked() {
            actions.push(Action::CardAlbums(albums.to_vec(), !on));
        }
        if widgets::icon_link(ui, Icon::Rename, "Edit tags").clicked() {
            actions.push(Action::EditTags(songs.clone()));
        }
    });
    ui.add_space(S2);
    for &(a, b) in albums {
        let artist = &app.index.artists[a];
        let al = &artist.albums[b];
        let w = ui.available_width();
        widgets::fitted(ui, RichText::new(al.title.as_deref().unwrap_or("Loose tracks")).italics().color(ink.text), w, Sense::hover());
        widgets::fitted(ui, RichText::new(&artist.name).size(13.0).color(ink.muted), w, Sense::hover());
        ui.add_space(S1 / 2.0);
    }
}
