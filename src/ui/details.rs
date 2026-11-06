//! The details of what's selected in the library, as Calibre shows a book's beside its list: the
//! album's cover, what it is, where it stands with the card, what can be done with it, and its
//! songs. With nothing selected, what's playing. Beside the library where there's room for it,
//! else as a sheet over the page (right-click › Details).

use egui::{Align2, FontId, RichText, ScrollArea, Sense, Ui, vec2};

use super::actions::Action;
use super::icons::Icon;
use super::layout::{ROW, S1, S2, S3, row, font};
use super::library::{Mark, album_status};
use super::text::{self, plural};
use super::theme::ink;
use super::widgets;
use crate::app::{App, LyricsState};

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

/// What an album's panel showed, for what's pinned over it.
struct Shown {
    title: String,
    /// Where *Songs | Lyrics* are, on the screen.
    tabs: egui::Rect,
    /// Lyrics in time with the song playing.
    in_time: bool,
}

/// The panel's contents, in a column `WIDTH` wide.
pub fn show(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let width = ui.available_width();
    let top_id = egui::Id::unique("details back to the top");
    let mut area = ScrollArea::vertical().id_salt("details").auto_shrink([false, false]);
    if ui.data_mut(|d| d.remove_temp::<bool>(top_id)).is_some() {
        area = area.vertical_scroll_offset(0.0);
    }
    let out = area.show(ui, |ui| {
        ui.set_width(width);
        ui.set_max_width(width);
        match about(app) {
            About::Album(a, b, playing) => Some(album(app, ui, a, b, playing, actions)),
            About::Several(albums) => {
                several(app, ui, &albums, actions);
                None
            }
            About::Nothing => {
                ui.add_space(S3);
                ui.label(RichText::new("Select an album to see it here: its cover, its songs, and where it stands with the card.").color(ink.muted));
                None
            }
        }
    });
    let Some(shown) = out.inner else { return };
    let area = out.inner_rect;
    // Scrolled by hand (the wheel, or the scroll bar held): the lyrics stop pulling it back.
    let wheel = ui.rect_contains_pointer(area) && ui.input(|i| i.smooth_scroll_delta.y != 0.0);
    let bar = ui.input(|i| i.pointer.primary_down() && i.pointer.press_origin().is_some_and(|p| area.contains(p) && p.x > area.right() - S2));
    if app.details_lyrics && (wheel || bar) {
        app.lyrics_follow = false;
    }
    // Once *Songs | Lyrics* is scrolled away, the album and they stay at the top: a way back up
    // that the lyrics, following the song down, don't fight.
    if shown.tabs.top() < area.top() {
        let strip = egui::Rect::from_min_size(area.min, vec2(area.width(), ROW + S1));
        ui.painter().rect_filled(strip, 0.0, ink.bg);
        ui.painter().hline(strip.x_range(), strip.bottom(), egui::Stroke::new(1.0, ink.faint));
        let mut pinned = ui.new_child(egui::UiBuilder::new().max_rect(strip).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let tabs_w = widgets::text_width(&pinned, "Songs", font::BODY) + S2 + widgets::text_width(&pinned, "Lyrics", font::BODY) + S1;
        let up = widgets::icon_button(&mut pinned, Icon::Up, "Back to the top of the album").clicked();
        let w = pinned.available_width() - tabs_w - S2;
        let title = widgets::fitted(&mut pinned, RichText::new(&shown.title).family(super::theme::italic()).size(font::BODY).color(ink.text), w, Sense::click()).on_hover_text("back to the top of the album").on_hover_cursor(egui::CursorIcon::PointingHand);
        if up || title.clicked() {
            ui.data_mut(|d| d.insert_temp(top_id, true));
            app.lyrics_follow = false;
        }
        pinned.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(S1);
            tabs(app, ui, true);
        });
    }
    // Not following: a way back to the line being sung, at the foot of the panel.
    if app.details_lyrics && shown.in_time && !app.lyrics_follow {
        let words = "Follow the song";
        let w = S2 + super::layout::ICON + S1 + widgets::text_width(ui, words, font::UI) + S2;
        let at = egui::Rect::from_center_size(egui::pos2(area.center().x, area.bottom() - S2 - super::layout::CONTROL / 2.0), vec2(w, super::layout::CONTROL));
        let mut foot = ui.new_child(egui::UiBuilder::new().max_rect(at));
        if widgets::primary_with(&mut foot, Icon::Down, words).on_hover_text("back to the line being sung, and keep it in view").clicked() {
            app.lyrics_follow = true;
            ui.data_mut(|d| d.remove_temp::<Option<usize>>(egui::Id::unique("lyrics line followed")));
        }
    }
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

/// *Songs | Lyrics*: `reversed` when laid out right to left.
fn tabs(app: &mut App, ui: &mut Ui, reversed: bool) {
    let order = if reversed { [true, false] } else { [false, true] };
    for (n, lyrics) in order.into_iter().enumerate() {
        if n > 0 {
            ui.add_space(S2);
        }
        let r = widgets::tab(ui, if lyrics { "Lyrics" } else { "Songs" }, app.details_lyrics == lyrics);
        let r = if lyrics { r.on_hover_text("the words of the song playing (in time), or of another of the album's") } else { r };
        if r.clicked() {
            app.details_lyrics = lyrics;
            app.lyrics_follow = true;
        }
    }
}

fn album(app: &mut App, ui: &mut Ui, ai: usize, bi: usize, playing: bool, actions: &mut Vec<Action>) -> Shown {
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
    ui.add(egui::Label::new(RichText::new(&title).family(super::theme::italic()).size(font::DISPLAY).color(ink.text)).wrap());
    if widgets::link(ui, &artist_name).on_hover_text("this artist's albums, and more about them").clicked() {
        actions.push(Action::ShowArtist(ai));
    }
    let al = &app.index.artists[ai].albums[bi];
    let rows = app.album_rows(&crate::library::search::Query::default());
    let facts: Vec<String> = match rows.iter().find(|r| r.artist == ai && r.album == bi) {
        Some(r) => [r.year.map(|y| y.to_string()), Some(r.format.clone()), Some(plural(r.songs, "song")), Some(text::bytes(r.bytes))].into_iter().flatten().collect(),
        None => vec![plural(al.tracks.len(), "song")],
    };
    ui.label(RichText::new(facts.join(" · ")).size(font::UI).color(ink.muted));
    // Its genres, each leading to the genre's artists and albums.
    let genres = app.album_genres(ai, bi);
    if !genres.names.is_empty() {
        ui.add_space(S1 / 2.0);
        ui.horizontal_wrapped(|ui| {
            for g in &genres.names {
                if widgets::chip(ui, Icon::Discover, g, ink.text).on_hover_text("this genre's artists and albums").clicked() {
                    actions.push(Action::ShowGenre(g.clone()));
                }
            }
        });
        if genres.from_artist {
            widgets::help(ui, "the artist's genres, from MusicBrainz: no song of this album names one");
        }
    }
    let (mark, status) = album_status(app, &ck, &app.index.artists[ai], al);
    if let Some(m) = mark {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, ROW), Sense::hover());
            m.paint(ui.painter(), r.center(), 3.5, &ink);
            ui.label(RichText::new(&status).size(font::UI).color(if m == Mark::Removing { ink.warn } else { ink.muted }));
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
    // Its songs, or one song's lyrics.
    let tabs_at = row(ui, |ui| tabs(app, ui, false)).response.rect;
    ui.add_space(S1);
    let mut shown = Shown { title, tabs: tabs_at, in_time: false };
    if app.details_lyrics {
        shown.in_time = lyrics(app, ui, &tracks, actions);
        return shown;
    }
    let current = app.current();
    for (pos, &t) in tracks.iter().enumerate() {
        let track = &app.tracks[t];
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        widgets::row_background(ui, &resp, rect, false);
        let now = current == Some(t);
        let color = if now { ink.accent } else { ink.text };
        let y = rect.center().y;
        let number = if now { "▸".to_string() } else { track.track.map(|n| n.to_string()).unwrap_or_default() };
        ui.painter().text(egui::pos2(rect.left() + S2 + S1, y), Align2::RIGHT_CENTER, number, FontId::proportional(font::UI), if now { ink.accent } else { ink.muted });
        let dur = text::duration(track.duration);
        let dur_w = widgets::data_width(ui, &dur);
        let g = widgets::truncated(ui, track.display_title(), FontId::proportional(font::UI), color, rect.width() - S3 - S1 - dur_w - S2);
        ui.painter().galley(egui::pos2(rect.left() + S3 + S1, y - g.size().y / 2.0), g, color);
        ui.painter().text(egui::pos2(rect.right() - S1, y), Align2::RIGHT_CENTER, dur, widgets::data(), ink.muted);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, track.display_title()));
        let resp = resp.on_hover_text("double-click to play from here");
        if resp.double_clicked() {
            actions.push(Action::Play(tracks.clone(), pos));
        }
        resp.context_menu(|ui| super::menu::music(app, ui, super::menu::Of::Songs(vec![t]), actions, |_, _| {}));
    }
    shown
}

/// One of the album's songs' lyrics: the song playing (following the singing, a click on a
/// line going there), else the one chosen with the arrows, the one selected, or the first.
/// Whether they're in time with the song playing.
fn lyrics(app: &mut App, ui: &mut Ui, tracks: &[usize], actions: &mut Vec<Action>) -> bool {
    let ink = ink(ui);
    let current = app.current().filter(|c| tracks.contains(c));
    let selected = tracks.iter().copied().find(|t| app.selected.contains(t));
    let t = app.lyrics_song.filter(|t| tracks.contains(t)).or(current).or(selected).unwrap_or(tracks[0]);
    let pos = tracks.iter().position(|&x| x == t).unwrap_or(0);
    // Which song: its title between arrows to the one before and after.
    row(ui, |ui| {
        ui.add_enabled_ui(pos > 0, |ui| {
            if widgets::icon_button(ui, Icon::Back, "The song before").clicked() {
                app.lyrics_song = Some(tracks[pos - 1]);
            }
        });
        let title = app.tracks[t].display_title();
        let w = ui.available_width() - ROW - S1;
        widgets::fitted(ui, RichText::new(&title).size(font::BODY).color(if current == Some(t) { ink.accent } else { ink.text }), w, Sense::hover()).on_hover_text(&title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(pos + 1 < tracks.len(), |ui| {
                if widgets::icon_button(ui, Icon::Forward, "The song after").clicked() {
                    app.lyrics_song = Some(tracks[pos + 1]);
                }
            });
        });
    });
    ui.add_space(S1);
    let state = app.lyrics_of(t).clone();
    let (mut again, mut in_time_now) = (false, false);
    match state {
        LyricsState::Looking => {
            row(ui, |ui| {
                super::cassette::spinner(ui, 12.0);
                widgets::muted(ui, "looking for the lyrics…");
            });
        }
        LyricsState::None => {
            widgets::muted(ui, "No lyrics for this song, in it or on LRCLIB.");
            again = widgets::link(ui, "Try again").on_hover_text("ask LRCLIB once more (lyrics are added there over time)").clicked();
        }
        LyricsState::Failed(e) => {
            ui.add(egui::Label::new(RichText::new(format!("Couldn't look: {e}")).size(font::UI).color(ink.muted)).wrap());
            again = widgets::link(ui, "Try again").clicked();
        }
        LyricsState::Found(l) if l.instrumental && l.plain.is_empty() => {
            widgets::muted(ui, "Instrumental: no words to show.");
        }
        LyricsState::Found(l) => {
            let playing = current == Some(t);
            // What kind they are, said first: in time or not looks the same until a line lights.
            row(ui, |ui| {
                if l.synced.is_empty() {
                    widgets::help(ui, "Words only: these aren't in time with the song");
                } else if playing {
                    widgets::help(ui, "In time · click a line to go to it");
                } else {
                    widgets::help(ui, "In time while it plays ·");
                    if widgets::link(ui, "Play it").clicked() {
                        actions.push(Action::Play(tracks.to_vec(), pos));
                    }
                }
            });
            ui.add_space(S1);
            if l.synced.is_empty() {
                ui.add(egui::Label::new(RichText::new(&l.plain).size(font::UI).color(ink.text)).wrap());
            } else {
                in_time(app, ui, &l, t, playing);
                in_time_now = playing;
            }
            ui.add_space(S1);
            let from = match l.source {
                Some(crate::lyrics::Source::File) => "from the .lrc file beside the song",
                Some(crate::lyrics::Source::Tags) => "from the song's tags",
                _ => "from LRCLIB (lrclib.net)",
            };
            row(ui, |ui| {
                widgets::help(ui, from);
                if l.source == Some(crate::lyrics::Source::Lrclib) && widgets::link(ui, "Not right?").on_hover_text("look again on LRCLIB").clicked() {
                    again = true;
                }
            });
        }
    }
    if again {
        app.lyrics_again(t);
    }
    ui.add_space(S1);
    if app.busy.is_none() && widgets::icon_link(ui, Icon::Search, "Find lyrics for the whole album").on_hover_text("ahead of time, kept to read offline").clicked() {
        actions.push(Action::FindLyrics(tracks.to_vec()));
    }
    if in_time_now {
        // Room below, so *Follow the song* never sits over the last lines.
        ui.add_space(super::layout::CONTROL + S3);
    }
    in_time_now
}

/// Synced lyrics, line by line, with the breaks where nothing is sung. Playing, the line being
/// sung is in the accent, those sung in grey, and it stays in view; a break under way fills its
/// three dots and counts down to the words.
fn in_time(app: &mut App, ui: &mut Ui, l: &crate::lyrics::Lyrics, t: usize, playing: bool) {
    let ink = ink(ui);
    let st = app.audio.status();
    let length = if playing { st.len.filter(|d| !d.is_zero()).unwrap_or(app.tracks[t].duration) } else { app.tracks[t].duration };
    let parts = l.stretches(length);
    let now = if playing { crate::lyrics::stretch_at(&parts, st.pos) } else { None };
    let follow_id = egui::Id::unique("lyrics line followed");
    let followed = ui.data(|d| d.get_temp::<Option<usize>>(follow_id)).flatten();
    for (i, part) in parts.iter().enumerate() {
        let end = parts.get(i + 1).map_or(length, |n| n.at);
        let sung = now.is_some_and(|n| i < n);
        let r = match &part.words {
            Some(words) => {
                let color = if now == Some(i) { ink.accent } else if sung { ink.muted } else { ink.text };
                ui.add(egui::Label::new(RichText::new(words).size(font::BODY).color(color)).wrap().sense(if playing { Sense::click() } else { Sense::hover() }))
            }
            None => {
                let done = (now == Some(i)).then(|| (st.pos.saturating_sub(part.at).as_secs_f32() / end.saturating_sub(part.at).as_secs_f32().max(0.1)).min(1.0));
                let left = text::duration(end.saturating_sub(st.pos) + std::time::Duration::from_millis(999));
                let long = text::duration(end.saturating_sub(part.at));
                let words = match (done.is_some(), i == 0, i + 1 == parts.len()) {
                    (true, true, _) => format!("the words start in {left}"),
                    (true, _, true) => format!("no more words · {left} to the end"),
                    (true, ..) => format!("the next line in {left}"),
                    (false, true, _) => format!("{long} before the words"),
                    (false, _, true) => format!("{long} to the end, without words"),
                    (false, ..) => format!("{long} without words"),
                };
                // A short one is its dots (its length when pointed at); a long one says it.
                let told = end.saturating_sub(part.at) >= crate::lyrics::LONG_BREAK;
                let r = pause_row(ui, done, sung, told.then_some(words.as_str()), playing);
                if told { r } else { r.on_hover_text(format!("{long} without words")) }
            }
        };
        if playing {
            let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
            if r.clicked() {
                app.audio.seek(part.at);
                app.lyrics_follow = true;
            }
            if app.lyrics_follow && now == Some(i) && followed != Some(i) {
                r.scroll_to_me(Some(egui::Align::Center));
                ui.data_mut(|d| d.insert_temp(follow_id, Some(i)));
            }
        }
    }
}

/// A break in the lyrics: three dots, filled in the accent as it goes by (`done`, while it's
/// under way), and what it is in words beside them, if it's told.
fn pause_row(ui: &mut Ui, done: Option<f32>, sung: bool, words: Option<&str>, playing: bool) -> egui::Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), if playing { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let y = rect.center().y;
    for k in 0..3 {
        let c = egui::pos2(rect.left() + 4.0 + 10.0 * k as f32, y);
        let color = match done {
            // Under way, all three in the accent from the start, faint until they fill.
            Some(d) => ink.accent.gamma_multiply(0.35).lerp_to_gamma(ink.accent, (d * 3.0 - k as f32).clamp(0.0, 1.0)),
            None if sung => ink.faint,
            None => ink.muted,
        };
        p.circle_filled(c, 2.5, color);
    }
    if let Some(words) = words {
        let color = if done.is_some() { ink.accent } else { ink.muted };
        let g = p.layout_no_wrap(words.to_string(), FontId::proportional(font::META), color);
        p.galley(egui::pos2(rect.left() + 34.0, y - g.size().y / 2.0), g, color);
    }
    resp
}

fn several(app: &App, ui: &mut Ui, albums: &[(usize, usize)], actions: &mut Vec<Action>) {
    let ink = ink(ui);
    let songs: Vec<usize> = albums.iter().flat_map(|&(a, b)| app.index.artists[a].albums[b].tracks.iter().copied()).collect();
    let bytes: u64 = songs.iter().map(|&t| app.tracks[t].size).sum();
    ui.add_space(S1);
    ui.label(RichText::new(format!("{} selected", plural(albums.len(), "album"))).size(font::HEADING).color(ink.text));
    ui.label(RichText::new(format!("{} · {}", plural(songs.len(), "song"), text::bytes(bytes))).size(font::UI).color(ink.muted));
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
        widgets::fitted(ui, RichText::new(al.title.as_deref().unwrap_or("Loose tracks")).family(super::theme::italic()).color(ink.text), w, Sense::hover());
        widgets::fitted(ui, RichText::new(&artist.name).size(font::UI).color(ink.muted), w, Sense::hover());
        ui.add_space(S1 / 2.0);
    }
}
