//! Card tab: saved writes. Each is a named layout for a card: what goes on it, the names and
//! order it uses there, and a sync that only copies what changed.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use egui::{Align, Layout, RichText, ScrollArea, Sense, Stroke, Ui, vec2};

use super::navigator::describe_space;
use super::text::{self, plural};
use super::theme::{self, ink};
use super::icons::Icon;
use super::widgets::{self, caps, link, muted};
use crate::app::App;
use crate::card::adopt::{IMPORT_FOLDER, Survey, gather_folder};
use crate::card::{AlbumOrder, Rules, Target, Unplayable, album_folder, card_size, target};
use crate::library::Track;

pub fn show(app: &mut App, ui: &mut Ui) {
    if app.tracks.is_empty() {
        writes_bar(app, ui);
        ui.add_space(14.0);
        muted(ui, "Choose a source in the folders tab first.");
        return;
    }
    let sizes = Sizes::compute(app);
    // Pinned while the rest scrolls: how full the card gets and what Sync changes, in view
    // while choosing music below.
    theme::panel_frame(ui).inner_margin(egui::Margin::symmetric(18, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        capacity(app, ui, &sizes);
        ui.add_space(6.0);
        sync_row(app, ui);
    });
    ui.add_space(6.0);
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(10.0);
        writes_bar(app, ui);
        ui.add_space(14.0);
        problems(app, ui);
        comparison(app, ui);
        ui.add_space(22.0);
        selection(app, ui, &sizes);
        ui.add_space(22.0);
        playlists(app, ui);
        ui.add_space(22.0);
        layout_options(app, ui);
        ui.add_space(16.0);
    });
}

/// What Sync changes on the card, in words, or None if nothing.
pub fn pending_text(app: &App) -> Option<String> {
    let s = &app.card_state;
    let mut parts = Vec::new();
    if s.added > 0 {
        parts.push(format!("{} to add ({})", plural(s.added, "album"), text::bytes(s.add_bytes)));
    }
    if s.replaced > 0 {
        parts.push(format!("{} to swap for another copy", plural(s.replaced, "album")));
    }
    if s.removed > 0 {
        parts.push(format!("{} to take off", plural(s.removed, "album")));
    }
    if s.repeats > 0 {
        parts.push(format!("{} on the card twice to take off", plural(s.repeats, "song")));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn room(app: &App) -> Option<(crate::card::SpaceInfo, u64)> {
    app.card_room()
}

/// A hairline as long as the room, and over it a bar as long as what's needed; red past the end.
fn space_bar(ui: &mut Ui, need: u64, room: u64, width: f32) {
    let ink = ink(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 12.0), Sense::hover());
    let scale = width / room.max(need).max(1) as f32;
    let y = rect.center().y;
    ui.painter().hline(rect.left()..=rect.left() + room as f32 * scale, y, Stroke::new(1.0, ink.faint));
    let used = rect.left() + need.min(room) as f32 * scale;
    ui.painter().hline(rect.left()..=used, y, Stroke::new(3.0, ink.text));
    if need > room {
        ui.painter().hline(used..=rect.left() + need as f32 * scale, y, Stroke::new(3.0, ink.accent));
    }
}

/// Changes since the last write, and the buttons that compare and sync.
fn sync_row(app: &mut App, ui: &mut Ui) {
    let w = app.write();
    let ready = w.dest.is_some() && app.saved.source.is_some();
    let connected = w.dest.as_deref().is_some_and(|d| d.exists());
    let state = match pending_text(app) {
        Some(t) => format!("at the next sync: {t}"),
        None if w.last_written.is_none() && w.manifest.is_empty() => "nothing written to the card yet".into(),
        None => "the card has what you chose".into(),
    };
    ui.horizontal(|ui| {
        if !ready {
            muted(ui, "choose a destination below to sync");
            return;
        }
        ui.allocate_ui(vec2(ui.available_width() - 240.0, 0.0), |ui| {
            ui.add(egui::Label::new(RichText::new(state).color(ink(ui).text)).wrap());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            if !connected {
                muted(ui, "card not connected");
                return;
            }
            if app.busy.is_none() && widgets::primary_with(ui, Icon::Sync, "Sync").on_hover_text("copy what you added, take off what you removed, update what changed").clicked() {
                app.run_write();
            }
            if app.busy.is_none() && widgets::button_with(ui, Icon::Compare, "Compare").on_hover_text("look at the card: what would be copied and removed; changes nothing").clicked() {
                app.preview_write();
            }
        });
    });
}

/// The library's view of the card, one line above the list: how full it gets, what Sync changes,
/// and Sync.
pub fn strip(app: &mut App, ui: &mut Ui) {
    let w = app.write();
    if w.dest.is_none() {
        return;
    }
    let ink = ink(ui);
    let name = w.name.clone();
    let connected = w.dest.as_deref().is_some_and(|d| d.exists());
    let need = app.card_state.planned_bytes;
    let pending = pending_text(app);
    let can_sync = connected && pending.is_some() && app.busy.is_none();
    let frame = egui::Frame::new().stroke(Stroke::new(1.0, ink.faint)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(14, 8));
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let button_w = if can_sync { 100.0 } else { 0.0 };
            ui.allocate_ui(vec2(ui.available_width() - button_w, 0.0), |ui| {
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::hover());
                        super::icons::paint(ui.painter(), rect, Icon::Card, ink.text);
                        ui.label(RichText::new(&name).color(ink.text));
                        match room(app) {
                            Some((_, room)) => {
                                space_bar(ui, need, room, 120.0);
                                muted(ui, format!("{} chosen of {} room", text::bytes(need), text::bytes(room)))
                                    .on_hover_text("Room: what's free on the card plus what this write's own files take there.");
                                if need > room {
                                    ui.label(RichText::new(format!("{} over", text::bytes(need - room))).color(ink.accent));
                                }
                            }
                            None => {
                                muted(ui, format!("{} chosen · card not connected", text::bytes(need)));
                            }
                        }
                    });
                    if let Some(p) = &pending {
                        widgets::help(ui, format!("At the next sync: {p}."));
                    }
                });
            });
            if can_sync {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button_with(ui, Icon::Sync, "Sync").on_hover_text("copy what you added, take off what you removed").clicked() {
                        app.run_write();
                    }
                });
            }
        });
    });
}

/// What the last card operation couldn't do, one line per file, with the saved full list.
fn problems(app: &mut App, ui: &mut Ui) {
    let Some(p) = &app.problems else { return };
    let ink = ink(ui);
    let mut dismiss = false;
    theme::panel_frame(ui).show(ui, |ui| {
        ui.set_max_width(760.0);
        let n = p.list.len();
        ui.label(RichText::new(format!("{}: {}", p.what, plural(n, "problem"))).color(ink.accent));
        const SHOWN: usize = 8;
        for e in p.list.iter().take(SHOWN) {
            ui.add(egui::Label::new(RichText::new(e).size(12.5).color(ink.muted)).truncate());
        }
        if n > SHOWN {
            ui.collapsing(RichText::new(format!("{} more", n - SHOWN)).size(12.5).color(ink.muted), |ui| {
                for e in p.list.iter().skip(SHOWN).take(500) {
                    ui.add(egui::Label::new(RichText::new(e).size(12.5).color(ink.muted)).truncate());
                }
            });
        }
        ui.horizontal(|ui| {
            if let Some(log) = &p.log
                && widgets::icon_link(ui, Icon::Folder, "open the full list").on_hover_text(log.display().to_string()).clicked()
            {
                crate::platform::reveal(log);
            }
            dismiss = widgets::icon_link(ui, Icon::Close, "dismiss").clicked();
        });
    });
    if dismiss {
        app.problems = None;
    }
    ui.add_space(14.0);
}

fn writes_bar(app: &mut App, ui: &mut Ui) {
    let mut select = None;
    ui.horizontal_wrapped(|ui| {
        caps(ui, "writes");
        for (i, w) in app.saved.writes.iter().enumerate() {
            if widgets::tab(ui, &w.name, i == app.saved.active_write).clicked() {
                select = Some(i);
            }
            ui.add_space(4.0);
        }
        ui.add_space(12.0);
        if widgets::icon_link(ui, Icon::Add, "new").on_hover_text("a fresh layout with everything selected").clicked() {
            app.new_write(false);
        }
        if widgets::icon_link(ui, Icon::Copy, "duplicate").on_hover_text("same choices, e.g. for a second card").clicked() {
            app.new_write(true);
        }
        if app.saved.writes.len() > 1 && widgets::icon_link(ui, Icon::Trash, "delete").on_hover_text("forgets the layout; files on the card stay").clicked() {
            app.delete_write();
        }
    });
    if let Some(i) = select {
        app.select_write(i);
    }
    let ink = ink(ui);
    ui.horizontal(|ui| {
        let w = &mut app.saved.writes[app.saved.active_write];
        let old = w.name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut w.name).font(egui::FontId::proportional(22.0)).text_color(ink.text).frame(egui::Frame::NONE).desired_width(360.0));
        // Names identify writes for background replies; keep them unique and non-empty.
        if r.lost_focus() && (w.name.trim().is_empty() || app.saved.writes.iter().filter(|x| x.name == app.saved.writes[app.saved.active_write].name).count() > 1) {
            app.saved.writes[app.saved.active_write].name = old;
        }
    });
    let w = app.write();
    let last = match w.last_written {
        Some(t) => format!("last written {} · {} on the card from this write ({})", ago(t), plural(w.manifest.len(), "file"), text::bytes(w.manifest_bytes())),
        None if !w.manifest.is_empty() => format!("not written yet · {} on the card from taking it over ({})", plural(w.manifest.len(), "file"), text::bytes(w.manifest_bytes())),
        None => "never written".into(),
    };
    widgets::help(ui, last);
    let shown = w.dest.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not chosen".into());
    let has_dest = w.dest.is_some();
    ui.horizontal(|ui| {
        caps(ui, "destination");
        ui.add(egui::Label::new(shown).truncate());
        if widgets::icon_link(ui, Icon::Card, if has_dest { "change card" } else { "choose a card" }).clicked() {
            app.volumes = crate::card::volumes();
            app.picker = Some(crate::ui::folders::Picker::Card { browse: None, show_all: false });
        }
    });
}

fn ago(t: u64) -> String {
    let s = crate::card::now().saturating_sub(t);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

/// Bytes per artist (cluster key) and album id, counting only files this write would copy.
struct Sizes {
    artist: HashMap<String, u64>,
    album: HashMap<String, u64>,
    selected: u64,
    playlists: u64,
    /// What writing hi-res songs at CD quality would save on the selected music, when the
    /// write doesn't already.
    cd_saving: u64,
}

impl Sizes {
    fn compute(app: &App) -> Self {
        let w = app.write();
        let mut s = Sizes { artist: HashMap::new(), album: HashMap::new(), selected: 0, playlists: 0, cd_saving: 0 };
        let rules = w.options.rules();
        let cd = Rules { cd_quality: true, ..rules };
        for a in &app.index.artists {
            let ck = &app.index.clusters[a.cluster].key;
            for al in &a.albums {
                let id = al.id(ck);
                let bytes: u64 = al
                    .tracks
                    .iter()
                    .map(|&i| &app.tracks[i])
                    .filter_map(|t| target(t, rules).ok().map(|to| card_size(t, to)))
                    .sum();
                // Of several editions, only the one going on the card counts towards the artist.
                if al.edition.is_none() || w.custom.on_card(ck, al, &a.albums) {
                    *s.artist.entry(ck.clone()).or_default() += bytes;
                }
                if w.custom.includes(ck, al, &a.albums) {
                    s.selected += bytes;
                    if !rules.cd_quality {
                        let at_cd: u64 = al.tracks.iter().map(|&i| &app.tracks[i]).filter_map(|t| target(t, cd).ok().map(|to| card_size(t, to))).sum();
                        s.cd_saving += bytes.saturating_sub(at_cd);
                    }
                }
                s.album.insert(id, bytes);
            }
        }
        let ids: Vec<PathBuf> = app.tracks.iter().map(Track::id).collect();
        let by_path = ids.iter().map(PathBuf::as_path).zip(&app.tracks).collect();
        s.playlists = crate::card::playlist::planned(&w.chosen_playlists(&app.saved.playlists), &by_path, &w.playlists_folder, rules)
            .iter()
            .flat_map(|(_, copies, _)| copies.iter().map(|c| card_size(c.track, c.target)))
            .sum();
        s
    }
}

fn capacity(app: &mut App, ui: &mut Ui, sizes: &Sizes) {
    let ink = ink(ui);
    let w = app.write();
    let need = sizes.selected + sizes.playlists;
    let space = room(app);
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        super::icons::paint(ui.painter(), rect, Icon::Card, ink.text);
        ui.label(RichText::new(&w.name).size(17.0).color(ink.text));
        let mut s = format!("{} chosen", text::bytes(need));
        if sizes.playlists > 0 {
            s.push_str(&format!(" (playlist copies {})", text::bytes(sizes.playlists)));
        }
        if let Some((i, _)) = &space {
            s.push_str(&format!(" · card {}", describe_space(i)));
        }
        muted(ui, s);
    });
    let Some((_, room)) = space else { return };
    let adoptable = app.card_survey.as_ref().filter(|(n, _)| *n == w.name).map(|(_, s)| s.used_bytes()).unwrap_or(0);
    if adoptable > 0 {
        muted(ui, format!("counting the {} a take-over moves into place as already there", text::bytes(adoptable)));
    }
    space_bar(ui, need, room, ui.available_width().min(560.0));
    if need > room {
        ui.label(RichText::new(format!("doesn't fit: {} over. Take some artists or albums off below, or in the library.", text::bytes(need - room))).color(ink.accent).size(13.0));
        // Hi-res songs at CD quality may be enough on their own.
        if sizes.cd_saving > 0 {
            ui.horizontal_wrapped(|ui| {
                let enough = need - sizes.cd_saving <= room;
                muted(ui, format!("Writing hi-res FLAC at CD quality would save about {}{}.", text::bytes(sizes.cd_saving), if enough { ", enough to fit" } else { "" }));
                if widgets::icon_link(ui, Icon::Check, "do that").on_hover_text("16-bit, 44.1 or 48 kHz FLAC for songs above that; through earbuds on a portable player the difference isn't audible").clicked() {
                    app.write_mut().options.cd_quality = true;
                    app.refresh_on_card();
                }
            });
        }
    }
}

/// The last comparison with the card, and what else is on it.
fn comparison(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    if let Some((_, p)) = &app.write_preview {
        let converting = if p.to_convert > 0 { format!(", {} of them converted or cut", p.to_convert) } else { String::new() };
        let mut parts = vec![format!("{} to copy ({}{converting})", p.to_copy, text::bytes(p.copy_bytes)), format!("{} already there", p.unchanged)];
        if !p.to_remove.is_empty() {
            parts.push(format!("{} to remove", p.to_remove.len()));
        }
        if p.skipped > 0 {
            parts.push(format!("{} left out", p.skipped));
        }
        if p.playlist_files > 0 {
            parts.push(format!("{} playlist copies", p.playlist_files));
        }
        ui.label(format!("Compared with the card: {}", parts.join(" · ")));
        take_over(app, ui);
        let Some((_, p)) = &app.write_preview else { return };
        if !p.to_remove.is_empty() {
            ui.collapsing(RichText::new("files that will be removed").color(ink.muted).size(13.0), |ui| {
                for r in p.to_remove.iter().take(200) {
                    ui.label(RichText::new(r.display().to_string()).size(12.5).color(ink.muted));
                }
            });
        }
    }
}

/// Music already on the card that this write didn't put there: offer to check it against the
/// library, then to move the copies of library songs into place instead of copying them again.
fn take_over(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let Some((name, p)) = &app.write_preview else { return };
    let (foreign, foreign_bytes, can_undo) = (p.foreign, p.foreign_bytes, p.can_undo_take_over);
    // Counts and sizes, copied out so the buttons below can change the app.
    let surveyed = app.card_survey.as_ref().filter(|(n, _)| n == name).map(|(_, s)| {
        let c = |l: &[(PathBuf, u64)]| (l.len(), Survey::bytes(l));
        ((s.used.len(), s.used_bytes()), c(&s.spare), c(&s.versions), c(&s.unknown), c(&s.other))
    });
    let clutter = app.card_survey.as_ref().filter(|(n, _)| n == name).map(|(_, s)| s.clutter()).unwrap_or_default();
    if foreign == 0 && !can_undo {
        return;
    }
    ui.add_space(6.0);
    theme::panel_frame(ui).show(ui, |ui| {
        ui.set_max_width(760.0);
        match surveyed {
            None if foreign > 0 => {
                ui.label(format!(
                    "The card also has {} that this write didn't put there ({}). Songs you copied by hand can be moved into this layout instead of being copied again.",
                    plural(foreign, "file"),
                    text::bytes(foreign_bytes)
                ));
                ui.add_space(4.0);
                if app.busy.is_none() && widgets::button_with(ui, Icon::Compare, "Check them against the library").on_hover_text("reads a little of each file; changes nothing").clicked() {
                    app.survey_card();
                }
            }
            None => {}
            Some(((used, used_bytes), (spare, spare_bytes), (versions, versions_bytes), (unknown, unknown_bytes), (other, other_bytes))) => {
                let what = if used == 1 { "is an exact copy of a song" } else { "are exact copies of songs" };
                ui.label(RichText::new(format!("{} on the card {what} this write needs ({}).", plural(used, "song"), text::bytes(used_bytes))).color(ink.text));
                muted(ui, "They're moved into place (renamed on the card, instant) instead of copied again.");
                if spare > 0 {
                    let what = if spare == 1 { "is a copy of a library song" } else { "are copies of library songs" };
                    muted(ui, format!("{} {what} this write leaves out or converts ({}).", plural(spare, "file"), text::bytes(spare_bytes)));
                }
                if versions > 0 {
                    let what = if versions == 1 { "is another version of a song" } else { "are other versions of songs" };
                    muted(ui, format!(
                        "{} {what} in your library ({}): an MP3 where your library has FLAC, different tags… This write puts your library's version on the card.",
                        plural(versions, "file"),
                        text::bytes(versions_bytes)
                    ));
                }
                if unknown > 0 {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{} aren't in your library at all ({}).", plural(unknown, "song"), text::bytes(unknown_bytes))).color(ink.text))
                            .on_hover_text("Kept on the card either way. To keep them for good, copy them into your music folder first.");
                        // Room on the music folder's drive, so the copy can't fill it up.
                        let room = app.saved.source.as_deref().and_then(crate::card::space).map(|s| s.free);
                        let fits = room.is_none_or(|free| free > unknown_bytes + 1_000_000_000);
                        if app.busy.is_none() && fits {
                            let hint = format!(
                                "Copies them, with their covers, into “{}” in your music folder, keeping the card's folder names; the card isn't changed. Then the library is rescanned, and checking the card again lets a take-over move them into place too.",
                                IMPORT_FOLDER
                            );
                            if link(ui, "copy them into your music folder").on_hover_text(hint).clicked() {
                                app.import_from_card();
                            }
                        } else if !fits {
                            ui.label(RichText::new("not enough room in your music folder's drive to copy them").size(12.5).color(ink.accent));
                        }
                    });
                }
                if other > 0 {
                    muted(ui, format!("{} of other kinds: covers, logs, playlists, other devices' data… ({}).", plural(other, "file"), text::bytes(other_bytes)));
                }
                ui.add_space(4.0);
                let gather = format!("move everything else into one folder, “{}” (nothing is deleted)", gather_folder());
                ui.checkbox(&mut app.gather_on_take_over, gather)
                    .on_hover_text("Keeps the player's top level to this write's artists. Delete the folder yourself once you're happy; or untick to leave those files where they are.");
                if !clutter.is_empty() {
                    let (n, bytes): (usize, u64) = clutter.iter().fold((0, 0), |(a, b), (_, n, s)| (a + n, b + s));
                    let kinds: Vec<String> = clutter.iter().map(|(k, n, s)| format!("{k}: {} ({})", plural(*n, "file"), text::bytes(*s))).collect();
                    ui.checkbox(&mut app.delete_clutter_on_take_over, format!("delete clutter: {} ({}) of iTunes data, system files and rip logs; can't be undone", plural(n, "file"), text::bytes(bytes)))
                        .on_hover_text(format!(
                            "{}\n\nThe Echo Mini can't use these, and your library keeps its own. Covers, scans, booklets, playlists, CUE sheets and text files are never deleted.",
                            kinds.join("\n")
                        ));
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if app.busy.is_none() && used + spare + versions + unknown + other > 0 && widgets::primary_with(ui, Icon::Card, "Take over the card").on_hover_text("only moves files on the card; undo puts everything back").clicked() {
                        app.take_over_card();
                    }
                    if widgets::icon_link(ui, Icon::Close, "not now").clicked() {
                        app.card_survey = None;
                    }
                });
            }
        }
        if can_undo && app.busy.is_none() && widgets::icon_link(ui, Icon::Back, "undo taking over the card").on_hover_text("move the files back to where they were").clicked() {
            app.undo_take_over();
        }
    });
}

fn layout_options(app: &mut App, ui: &mut Ui) {
    let off_recho = crate::library::non_recho_summary(&app.tracks);
    let mut changed = false;
    let ink = ink(ui);
    let w = &mut app.saved.writes[app.saved.active_write];
    widgets::section(ui, Icon::Settings, "Folders and options");
    ui.add_space(8.0);
    widgets::setting(ui, "Album folders", |ui| {
        ui.horizontal(|ui| {
            changed |= ui.radio_value(&mut w.options.album_order, AlbumOrder::Year, "1997 - Album").changed();
            changed |= ui.radio_value(&mut w.options.album_order, AlbumOrder::Title, "Album").changed();
            changed |= ui.radio_value(&mut w.options.album_order, AlbumOrder::Custom, "01 - Album").changed();
        });
        widgets::help(ui, "By year (oldest first), by title, or numbered in your own order: use the arrows in the list above.");
    });
    widgets::setting(ui, "Other folders", |ui| {
        egui::Grid::new("folders").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            muted(ui, "songs without an album");
            changed |= ui.add(egui::TextEdit::singleline(&mut w.options.singles_folder).desired_width(160.0)).changed();
            ui.end_row();
            muted(ui, "playlists");
            changed |= ui.add(egui::TextEdit::singleline(&mut w.playlists_folder).desired_width(160.0)).changed();
            ui.end_row();
        });
    });
    if let Some((n, formats)) = &off_recho {
        widgets::setting(ui, "Formats RECHO can't play", |ui| {
            ui.label(RichText::new(format!("{}: {formats}", plural(*n, "song"))).color(ink.text));
            ui.add_space(2.0);
            changed |= ui
                .radio_value(&mut w.options.unplayable, Unplayable::Convert, "convert them")
                .on_hover_text("Takes a while the first time; later writes keep what's already on the card.")
                .changed();
            if w.options.unplayable == Unplayable::Convert {
                // What converting would make of them, and what still can't go (no ffmpeg, AC-4…).
                let (mut flac, mut mp3, mut left) = (0, 0, BTreeMap::<&str, usize>::new());
                let rules = Rules { unplayable: Unplayable::Convert, cd_quality: w.options.cd_quality };
                for t in app.tracks.iter().filter(|t| !t.on_recho() && t.cue.is_none()) {
                    match target(t, rules) {
                        Ok(Target::Flac | Target::FlacCd) => flac += 1,
                        Ok(Target::Mp3) => mp3 += 1,
                        Ok(Target::Copy) => {}
                        Err(why) => *left.entry(why).or_default() += 1,
                    }
                }
                let mut made = Vec::new();
                if flac > 0 {
                    made.push(format!("{} to FLAC (lossless ones, nothing lost)", plural(flac, "song")));
                }
                if mp3 > 0 {
                    made.push(format!("{} to MP3 320 (AAC, Dolby and other lossy ones, in stereo)", plural(mp3, "song")));
                }
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.vertical(|ui| {
                        for m in made {
                            widgets::help(ui, m);
                        }
                        for (why, n) in left {
                            ui.label(RichText::new(format!("{} left out: {why}.", plural(n, "song"))).color(ink.accent).size(13.0));
                        }
                    });
                });
            }
            changed |= ui.radio_value(&mut w.options.unplayable, Unplayable::Leave, "leave them off the card").changed();
            changed |= ui.radio_value(&mut w.options.unplayable, Unplayable::Copy, "copy them anyway").changed();
            if w.options.unplayable == Unplayable::Copy {
                theme::warning_frame(ui).show(ui, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} will be copied but won't play on the Echo Mini: RECHO plays FLAC, MP3 and WAV only. They'll take {} of space and show up in folders as files that do nothing.",
                            plural(*n, "file"),
                            text::bytes(app.tracks.iter().filter(|t| !t.on_recho()).map(|t| t.size).sum())
                        ))
                        .color(ink.accent),
                    );
                });
            }
        });
    }
    widgets::setting(ui, "Hi-res songs", |ui| {
        changed |= ui.checkbox(&mut w.options.cd_quality, "write them at CD quality to save space").changed();
        widgets::help(ui, "16-bit, 44.1 or 48 kHz FLAC instead of 24-bit at 96 or 192 kHz: a third to a sixth of the space. Through earbuds the difference isn't audible; your music folder keeps the originals.");
    });
    widgets::setting(ui, "Housekeeping", |ui| {
        changed |= ui.checkbox(&mut w.options.write_tags, "write corrected tags into the copies").changed();
        changed |= ui.checkbox(&mut w.prune, "remove what this write put on the card that's no longer chosen").changed();
        changed |= ui.checkbox(&mut w.options.clean_mac_files, "delete macOS ._ and .DS_Store files on the card").changed();
    });
    if changed {
        app.write_preview = None;
    }
}

fn playlists(app: &mut App, ui: &mut Ui) {
    widgets::section(ui, Icon::Playlists, "Playlists on this card");
    if app.saved.playlists.is_empty() {
        widgets::help(ui, "None yet; make some in the playlists tab.");
        return;
    }
    let ids: Vec<PathBuf> = app.tracks.iter().map(Track::id).collect();
    let by_path = ids.iter().map(PathBuf::as_path).zip(&app.tracks).collect();
    let mut changed = false;
    let w = &mut app.saved.writes[app.saved.active_write];
    ui.horizontal_wrapped(|ui| {
        for p in &app.saved.playlists {
            let mut on = w.playlists.contains(&p.name);
            let label = format!("{} ({}, {})", p.name, p.tracks.len(), text::bytes(p.bytes(&by_path)));
            if ui.checkbox(&mut on, label).changed() {
                changed = true;
                if on {
                    w.playlists.insert(p.name.clone());
                } else {
                    w.playlists.remove(&p.name);
                }
            }
        }
    });
    if changed {
        app.write_preview = None;
    }
}

enum Edit {
    /// Artist and album, by index; for one of several editions, on picks that edition.
    Artist(usize, bool),
    Album(usize, usize, bool),
    All(bool),
    Expand(String),
    Rename(bool, String, String),
    StartRename(bool, String, String),
    Move(String, Vec<String>, usize, isize),
}

fn selection(app: &mut App, ui: &mut Ui, sizes: &Sizes) {
    let ink = ink(ui);
    let mut edits = Vec::new();
    widgets::section(ui, Icon::Library, "What goes on the card");
    widgets::help(ui, "Tick what goes on the card, here or in the library; Sync copies it. On the right, the folder names on the card; rename changes them for this write only.");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        super::icons::paint(ui.painter(), rect, Icon::Search, ink.muted);
        ui.add(egui::TextEdit::singleline(&mut app.card_filter).hint_text("filter").desired_width(200.0));
        if widgets::icon_link(ui, Icon::Add, "tick all").clicked() {
            edits.push(Edit::All(true));
        }
        if widgets::icon_link(ui, Icon::Remove, "untick all").clicked() {
            edits.push(Edit::All(false));
        }
    });
    ui.add_space(4.0);
    let w = app.write();
    let needle = app.card_filter.to_lowercase();
    let custom_order = w.options.album_order == AlbumOrder::Custom;
    for (ai, artist) in app.index.artists.iter().enumerate() {
        let ck = &app.index.clusters[artist.cluster].key;
        if !needle.is_empty()
            && !artist.name.to_lowercase().contains(&needle)
            && !artist.albums.iter().any(|a| a.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&needle)))
        {
            continue;
        }
        // Ticked when everything goes, half when some does.
        let chosen: Vec<bool> = artist.albums.iter().map(|al| w.custom.includes(ck, al, &artist.albums)).collect();
        let (any, all) = (chosen.iter().any(|&c| c), chosen.iter().all(|&c| c));
        let whole = w.custom.whole_artist(ck);
        let card_name = w.custom.artist_names.get(ck).cloned().unwrap_or_else(|| artist.name.clone());
        let open = app.card_expanded.contains(ck);
        ui.horizontal(|ui| {
            let mut on = all && any;
            let hint = if whole { "the whole artist goes on the card, albums added later too" } else { "tick to put the whole artist on the card, albums added later too" };
            if ui.add(egui::Checkbox::without_text(&mut on).indeterminate(any && !all)).on_hover_text(hint).changed() {
                edits.push(Edit::Artist(ai, on));
            }
            let name = RichText::new(format!("{} {}", if open { "▾" } else { "▸" }, artist.name)).size(16.0).color(if any { ink.text } else { ink.muted });
            // Leave room for the size and the "rename" link on the right.
            let r = widgets::fitted(ui, name, ui.available_width() - 200.0, Sense::click()).on_hover_text(&artist.name);
            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                edits.push(Edit::Expand(ck.clone()));
            }
            muted(ui, text::bytes(sizes.artist.get(ck).copied().unwrap_or(0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Clear of the scrollbar.
                ui.add_space(10.0);
                rename_widget(ui, &app.card_renaming, true, ck, &card_name, &mut edits);
            });
        });
        if !open {
            continue;
        }
        let ordered = w.custom.ordered_albums(ck, &artist.albums);
        let order_ids: Vec<String> = ordered.iter().map(|a| a.id(ck)).collect();
        let positions = w.custom.positions(ck, &ordered);
        for (n, al) in ordered.iter().enumerate() {
            let id = &order_ids[n];
            let title = w.custom.album_name(ck, al);
            let folder = album_folder(al, title, positions[n], &w.options);
            let mut on = w.custom.includes(ck, al, &artist.albums);
            let on_card = on;
            let bi = artist.albums.iter().position(|a| std::ptr::eq(a, *al)).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                let hint = if al.edition.is_some() { "One edition of this album goes on the card; ticking this one replaces the other." } else { "" };
                let r = ui.add(egui::Checkbox::without_text(&mut on));
                if (if hint.is_empty() { r } else { r.on_hover_text(hint) }).changed() {
                    edits.push(Edit::Album(ai, bi, on));
                }
                let shown = al.title.as_deref().unwrap_or("Loose tracks");
                let title_text = RichText::new(shown).italics().color(if on_card { ink.text } else { ink.muted });
                let title_w = ui.available_width() * if al.edition.is_some() { 0.25 } else { 0.45 };
                widgets::fitted(ui, title_text, title_w, Sense::hover()).on_hover_text(shown);
                if let Some(e) = &al.edition {
                    let mut hover = format!("edition {} of {}: {} · {}", e.rank + 1, e.count, e.label, e.format);
                    if let Some(s) = &e.superseded {
                        hover.push_str(&format!("\nEdition {} has all these songs in {} quality.", s.rank + 1, if s.higher { "higher" } else { "the same" }));
                    }
                    ui.label(RichText::new(format!("{} · {}", text::shorten(&e.label, 30), e.format)).size(12.5).color(ink.muted)).on_hover_text(hover);
                }
                muted(ui, format!("{} · {}", plural(al.tracks.len(), "track"), text::bytes(sizes.album.get(id).copied().unwrap_or(0))));
                // Songs RECHO can't read as they are: converted, copied anyway, or left out.
                let fates: Vec<Result<Target, &str>> = al.tracks.iter().map(|&i| &app.tracks[i]).filter(|t| !t.on_recho() && t.cue.is_none()).map(|t| target(t, w.options.rules())).collect();
                let converted = fates.iter().filter(|f| matches!(f, Ok(Target::Flac | Target::FlacCd | Target::Mp3))).count();
                let copied = fates.iter().filter(|f| matches!(f, Ok(Target::Copy))).count();
                let left: Vec<&str> = al.tracks.iter().filter_map(|&i| target(&app.tracks[i], w.options.rules()).err()).collect();
                if converted > 0 {
                    muted(ui, format!("{converted} converted")).on_hover_text("RECHO plays FLAC, MP3 and WAV only; these are converted on the way (lossless to FLAC, the rest to MP3 320)");
                }
                if copied > 0 {
                    ui.label(RichText::new(format!("{copied} won't play")).size(12.5).color(ink.accent)).on_hover_text("copied as they are, but RECHO plays FLAC, MP3 and WAV only; change this under layout below");
                }
                if let Some(why) = left.first() {
                    ui.label(RichText::new(format!("{} left out", left.len())).size(12.5).color(ink.accent)).on_hover_text(*why);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(10.0);
                    if custom_order && al.title.is_some() {
                        if widgets::icon_button(ui, Icon::Down).on_hover_text("move down").clicked() {
                            edits.push(Edit::Move(ck.clone(), order_ids.clone(), n, 1));
                        }
                        if widgets::icon_button(ui, Icon::Up).on_hover_text("move up").clicked() {
                            edits.push(Edit::Move(ck.clone(), order_ids.clone(), n, -1));
                        }
                    }
                    if al.title.is_some() {
                        rename_widget(ui, &app.card_renaming, false, id, title.unwrap_or(al.title.as_deref().unwrap_or_default()), &mut edits);
                    }
                    let artist_dir = crate::library::names::sanitize_component(&card_name, 80);
                    let path = format!("{artist_dir} / {folder}");
                    // Whatever space is left between the album and the links, and no more.
                    ui.add(egui::Label::new(RichText::new(text::shorten(&path, 50)).size(12.5).color(ink.muted)).truncate()).on_hover_text(path);
                });
            });
        }
    }

    if edits.is_empty() {
        return;
    }
    let mut touched_layout = false;
    for e in edits {
        let w = &mut app.saved.writes[app.saved.active_write];
        match e {
            Edit::Expand(k) => {
                if !app.card_expanded.remove(&k) {
                    app.card_expanded.insert(k);
                }
            }
            Edit::StartRename(is_artist, key, current) => app.card_renaming = Some((is_artist, key, current)),
            Edit::Rename(is_artist, key, name) => {
                let map = if is_artist { &mut w.custom.artist_names } else { &mut w.custom.album_names };
                if name.trim().is_empty() {
                    map.remove(&key);
                } else {
                    map.insert(key, name.trim().to_string());
                }
                app.card_renaming = None;
                touched_layout = true;
            }
            Edit::Artist(ai, on) => app.card_artist(ai, on),
            Edit::Album(ai, bi, on) => app.card_album(ai, bi, on),
            Edit::All(on) => {
                for a in &app.index.artists {
                    w.custom.set_artist(&app.index.clusters[a.cluster].key, &a.albums, on);
                }
                touched_layout = true;
            }
            Edit::Move(ck, mut ids, n, d) => {
                let m = n as isize + d;
                if m >= 0 && (m as usize) < ids.len() {
                    ids.swap(n, m as usize);
                    w.custom.album_order.insert(ck, ids);
                    touched_layout = true;
                }
            }
        }
    }
    if touched_layout {
        app.chosen_changed();
    }
}

/// "rename" link, or the inline editor when this row is being renamed.
fn rename_widget(ui: &mut Ui, renaming: &Option<(bool, String, String)>, is_artist: bool, key: &str, current: &str, edits: &mut Vec<Edit>) {
    match renaming {
        Some((a, k, buf)) if *a == is_artist && k == key => {
            let mut text = buf.clone();
            if widgets::icon_link(ui, Icon::Check, "save").clicked() {
                edits.push(Edit::Rename(is_artist, key.to_string(), text.clone()));
            }
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(200.0));
            if r.changed() {
                edits.push(Edit::StartRename(is_artist, key.to_string(), text.clone()));
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                edits.push(Edit::Rename(is_artist, key.to_string(), text));
            }
        }
        _ => {
            if widgets::icon_link(ui, Icon::Rename, "rename").on_hover_text("folder name on this card only; empty resets").clicked() {
                edits.push(Edit::StartRename(is_artist, key.to_string(), current.to_string()));
            }
        }
    }
}
