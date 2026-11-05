//! Card tab: saved writes. Each is a named layout for a card: what goes on it, the names and
//! order it uses there, and a sync that only copies what changed.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Align, Layout, RichText, ScrollArea, Sense, Stroke, Ui, vec2};

use super::navigator::describe_space;
use super::text::{self, ago, plural};
use super::theme::{self, ink};
use super::icons::Icon;
use super::library::{Mark, album_status};
use super::widgets::{self, caps, link, muted};
use crate::app::App;
use crate::card::adopt::{IMPORT_FOLDER, Survey, gather_folder};
use crate::card::pattern::{Field, Pattern, default_for};
use crate::card::{AlbumOrder, Device, Rules, Target, Unplayable, card_size, song_path, target};
use crate::library::Track;
use crate::library::search::fold;
use super::layout::{ICON, S1, S2, S3, row, row_wrapped};

pub fn show(app: &mut App, ui: &mut Ui) {
    if app.tracks.is_empty() {
        writes_bar(app, ui);
        ui.add_space(S2);
        let why = if app.saved.source.is_none() {
            "Choose your music folder in the folders tab first."
        } else if app.source_missing {
            "Can't reach the music folder: is its drive connected? The card waits until it's back."
        } else if app.busy.is_some() {
            "Reading the library…"
        } else {
            "The music folder holds no songs yet."
        };
        muted(ui, why);
        return;
    }
    let sizes = Sizes::cached(app, ui);
    // Pinned while the rest scrolls: how full the card gets and what Sync changes, in view
    // while choosing music below.
    theme::panel_frame(ui).inner_margin(egui::Margin::same(S2 as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        capacity(app, ui, &sizes);
        ui.add_space(S1);
        sync_row(app, ui);
    });
    ui.add_space(S1);
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(S2);
        writes_bar(app, ui);
        ui.add_space(S2);
        problems(app, ui);
        comparison(app, ui);
        ui.add_space(S3);
        selection(app, ui, &sizes);
        ui.add_space(S3);
        playlists(app, ui);
        ui.add_space(S3);
        layout_options(app, ui);
        ui.add_space(S2);
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
    if s.renamed > 0 {
        parts.push(format!("{} to rename on the card", plural(s.renamed, "song")));
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
    ui.painter().hline(rect.left()..=used, y, Stroke::new(3.0, ink.accent));
    if need > room {
        ui.painter().hline(used..=rect.left() + need as f32 * scale, y, Stroke::new(3.0, ink.warn));
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
    let unavailable = ready.then(|| app.library_unavailable()).flatten();
    row(ui, |ui| {
        if !ready {
            muted(ui, "choose a destination below to sync");
            return;
        }
        if let Some(why) = unavailable {
            ui.label(RichText::new(format!("Sync waits: {why}")).color(ink(ui).warn));
            return;
        }
        ui.allocate_ui(vec2(ui.available_width() - 240.0, 0.0), |ui| {
            ui.add(egui::Label::new(RichText::new(state).color(ink(ui).text)).wrap());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(S2);
            if !connected {
                muted(ui, "card not connected");
                return;
            }
            if app.busy.is_none() && app.refused_by_label().is_none() && widgets::primary_with(ui, Icon::Sync, "Sync").on_hover_text(format!("copy what you added, take off what you removed, update what changed ({})", ui.ctx().format_shortcut(&super::keys::SYNC))).clicked() {
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
    let can_sync = connected && pending.is_some() && app.busy.is_none() && app.refused_by_label().is_none() && app.library_unavailable().is_none();
    let frame = egui::Frame::new().stroke(Stroke::new(1.0, ink.faint)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(S2 as i8, S1 as i8));
    frame.show(ui, |ui| {
        row(ui, |ui| {
            let button_w = if can_sync { 100.0 } else { 0.0 };
            ui.allocate_ui(vec2(ui.available_width() - button_w, 0.0), |ui| {
                ui.vertical(|ui| {
                    row_wrapped(ui, |ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::hover());
                        super::icons::paint(ui.painter(), rect, Icon::Card, ink.text);
                        ui.label(RichText::new(&name).color(ink.text));
                        match room(app) {
                            Some((_, room)) => {
                                space_bar(ui, need, room, 120.0);
                                muted(ui, format!("{} chosen of {} room", text::bytes(need), text::bytes(room)))
                                    .on_hover_text("Room: what's free on the card plus what this write's own files take there.");
                                if need > room {
                                    ui.label(RichText::new(format!("{} over", text::bytes(need - room))).color(ink.warn));
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
                    if widgets::primary_with(ui, Icon::Sync, "Sync").on_hover_text(format!("copy what you added, take off what you removed ({})", ui.ctx().format_shortcut(&super::keys::SYNC))).clicked() {
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
        ui.label(RichText::new(format!("{}: {}", p.what, plural(n, "problem"))).color(ink.warn));
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
        row(ui, |ui| {
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
    ui.add_space(S2);
}

fn writes_bar(app: &mut App, ui: &mut Ui) {
    let mut select = None;
    row_wrapped(ui, |ui| {
        caps(ui, "writes");
        for (i, w) in app.saved.writes.iter().enumerate() {
            if widgets::tab(ui, &w.name, i == app.saved.active_write).clicked() {
                select = Some(i);
            }
            ui.add_space(S1);
        }
        ui.add_space(S2);
        if widgets::icon_link(ui, Icon::Add, "new").on_hover_text("a fresh layout with everything selected").clicked() {
            app.new_write(false);
        }
        if widgets::icon_link(ui, Icon::Copy, "duplicate").on_hover_text("same choices, e.g. for a second card").clicked() {
            app.new_write(true);
        }
        if app.saved.writes.len() > 1 && widgets::icon_link(ui, Icon::Trash, "delete").on_hover_text("forgets this setup; asks first").clicked() {
            app.confirm = Some(crate::app::Confirm::DeleteWrite(app.write().id.clone()));
        }
    });
    if let Some(i) = select {
        app.select_write(i);
    }
    let ink = ink(ui);
    row(ui, |ui| {
        let w = &mut app.saved.writes[app.saved.active_write];
        let old = w.name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut w.name).align(egui::Align2::LEFT_CENTER).font(egui::FontId::proportional(22.0)).text_color(ink.text).frame(egui::Frame::NONE).desired_width(360.0));
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
    row(ui, |ui| {
        caps(ui, "destination");
        // The path gives way to the button, shortened with "…"; all of it shows on hover.
        let button_w = widgets::text_width(ui, "Change card…", 14.5) + ICON + S1 + 2.0 * S2 + S1;
        widgets::fitted(ui, RichText::new(&shown), ui.available_width() - button_w, Sense::hover()).on_hover_text(&shown);
        if widgets::button_with(ui, Icon::Card, if has_dest { "Change card…" } else { "Choose card…" }).clicked() {
            app.volumes = crate::card::volumes();
            app.picker = Some(crate::ui::folders::Picker::Card { browse: None, show_all: false });
        }
    });
    label_note(app, ui);
    // The player decides what's converted, what's warned about, and how much fits.
    let mut device = app.write().options.device;
    row_wrapped(ui, |ui| {
        caps(ui, "player");
        egui::ComboBox::from_id_salt("player").selected_text(device.name()).show_ui(ui, |ui| {
            for d in Device::ALL {
                ui.selectable_value(&mut device, d, d.name()).on_hover_text(d.about());
            }
        });
        muted(ui, device.about());
        let answered = app.write().options.tested.get(&app.write().options.device).map_or(0, |r| r.len());
        let test = if answered > 0 { format!("tested ({answered} answers)") } else { "test it on the player".into() };
        if widgets::icon_link(ui, Icon::Check, test).on_hover_text("write a folder of samples to the card, try them, and tick what works").clicked() {
            app.player_test_open = true;
        }
    });
    if device != app.write().options.device {
        app.write_mut().options.device = device;
        app.refresh_on_card();
    }
}

/// A Sync that would take most of the setup's songs off the card: say so, and go ahead only on a yes.
pub fn sync_confirm_dialog(app: &mut App, ui: &mut Ui) {
    let Some((id, removing, songs)) = app.sync_confirm.clone() else { return };
    if id != app.write().id {
        app.sync_confirm = None;
        return;
    }
    let ink = ink(ui);
    let frame = super::theme::dialog_frame(ui);
    let mut answer = None;
    let r = egui::Modal::new(egui::Id::unique("sync confirm")).frame(frame).show(ui.ctx(), |ui| {
        ui.set_width(520.0f32.min(ui.ctx().content_rect().width() - 80.0));
        ui.label(RichText::new(format!("Take {removing} of {songs} songs off the card?")).size(18.0).color(ink.text));
        ui.add_space(S1);
        widgets::help(
            ui,
            format!(
                "This Sync removes most of what “{}” put on the card: everything no longer chosen. If you meant to choose less music, go ahead. If the library looks incomplete (a drive not connected, a folder moved), cancel and check it first.",
                app.write().name
            ),
        );
        ui.add_space(S2);
        row(ui, |ui| {
            if widgets::primary_with(ui, Icon::Sync, "Take them off and sync").clicked() {
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
        Some(true) => app.sync_confirmed(),
        Some(false) => app.sync_confirm = None,
        None => {}
    }
}

/// When the card's label names another setup: whose card it is, and what to do about it.
fn label_note(app: &mut App, ui: &mut Ui) {
    let Some(l) = app.card_owner().cloned() else { return };
    let ink = ink(ui);
    let this = app.write().name.clone();
    match app.owner_setup() {
        Some(i) => {
            ui.label(RichText::new(format!("This card belongs to “{}” (written {}), so “{this}” won't sync to it or take it over.", l.name, ago(l.written))).color(ink.warn));
            row_wrapped(ui, |ui| {
                if widgets::icon_link(ui, Icon::Card, format!("switch to “{}”", l.name)).clicked() {
                    app.select_write(i);
                }
                if widgets::icon_link(ui, Icon::Check, "use it for this setup")
                    .on_hover_text(format!("relabels the card for “{this}”; “{}” won't sync to it any more, and the files it put there stay", l.name))
                    .clicked()
                {
                    app.claim_card();
                }
            });
        }
        None => {
            widgets::help(
                ui,
                format!("Last written by Pausetape for “{}”, a card setup not on this Mac ({}). Sync makes it “{this}”'s; it only ever removes files “{this}” put there.", l.name, l.device.name()),
            );
        }
    }
}

/// A song from the library named by `pattern`: the first album with a year and numbered
/// tracks, as "Artist / 1997 - Album / 02 - Song.flac".
fn pattern_example(index: &crate::library::grouping::Index, tracks: &[crate::library::Track], w: &crate::card::WriteProfile, pattern: &Pattern) -> Option<String> {
    let (artist, album) = index.artists.iter().flat_map(|a| a.albums.iter().map(move |al| (a, al))).find(|(_, al)| al.year.is_some() && al.tracks.len() > 1 && al.tracks.iter().all(|&i| tracks[i].track.is_some()))?;
    let ck = &index.clusters[artist.cluster].key;
    let t = &tracks[*album.tracks.get(1)?];
    let name = w.custom.artist_names.get(ck).unwrap_or(&artist.name);
    let rel = song_path(pattern, t, album, name, w.custom.album_name(ck, album), 0, &w.options, &t.ext);
    Some(rel.iter().map(|c| c.to_string_lossy()).collect::<Vec<_>>().join(" / "))
}

/// Bytes per artist (cluster key) and album id, counting only files this write would copy.
struct Sizes {
    artist: HashMap<String, u64>,
    album: HashMap<String, u64>,
    selected: u64,
    playlists: u64,
    /// Songs the write would put on the card, playlist copies included.
    songs: usize,
    /// What writing hi-res songs at CD quality would save on the selected music, when the
    /// write doesn't already.
    cd_saving: u64,
    /// The same for writing every lossless song as MP3 320.
    mp3_saving: u64,
    /// Artists with music chosen, largest first: (artist, albums chosen, bytes chosen).
    largest: Vec<(usize, usize, u64)>,
}

impl Sizes {
    /// `compute`, redone only when something it depends on has changed: it works out every
    /// song's size on the card, far too much for every frame of a large library (about 0.2 s
    /// for 7,000 songs in a debug build).
    fn cached(app: &App, ui: &Ui) -> Arc<Self> {
        use std::hash::{Hash, Hasher};
        let w = app.write();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        app.library_generation.hash(&mut h);
        serde_json::to_vec(&(&w.id, &w.options, &w.custom, &w.playlists, &w.playlists_folder)).unwrap_or_default().hash(&mut h);
        for p in &app.saved.playlists {
            (&p.name, &p.tracks).hash(&mut h);
        }
        let key = h.finish();
        let id = egui::Id::unique("card sizes");
        if let Some((k, s)) = ui.ctx().data(|d| d.get_temp::<(u64, Arc<Sizes>)>(id))
            && k == key
        {
            return s;
        }
        let s = Arc::new(Self::compute(app));
        ui.ctx().data_mut(|d| d.insert_temp(id, (key, s.clone())));
        s
    }

    fn compute(app: &App) -> Self {
        let w = app.write();
        let mut s = Sizes { artist: HashMap::new(), album: HashMap::new(), selected: 0, playlists: 0, songs: 0, cd_saving: 0, mp3_saving: 0, largest: Vec::new() };
        let rules = w.options.rules();
        let cd = Rules { cd_quality: true, ..rules };
        let mp3 = Rules { lossless_mp3: true, ..rules };
        for (ai, a) in app.index.artists.iter().enumerate() {
            let ck = &app.index.clusters[a.cluster].key;
            for al in &a.albums {
                let id = al.id(ck);
                let under = |rules| -> u64 { al.tracks.iter().map(|&i| &app.tracks[i]).filter_map(|t| target(t, rules).ok().map(|to| card_size(t, to))).sum() };
                let bytes = under(rules);
                // Of several editions, only the one going on the card counts towards the artist.
                if al.edition.is_none() || w.custom.on_card(ck, al, &a.albums) {
                    *s.artist.entry(ck.clone()).or_default() += bytes;
                }
                if w.custom.includes(ck, al, &a.albums) {
                    s.selected += bytes;
                    s.songs += al.tracks.iter().filter(|&&i| target(&app.tracks[i], rules).is_ok()).count();
                    match s.largest.last_mut() {
                        Some((a, n, b)) if *a == ai => (*n, *b) = (*n + 1, *b + bytes),
                        _ => s.largest.push((ai, 1, bytes)),
                    }
                    if !rules.cd_quality && !rules.lossless_mp3 {
                        s.cd_saving += bytes.saturating_sub(under(cd));
                    }
                    if !rules.lossless_mp3 {
                        s.mp3_saving += bytes.saturating_sub(under(mp3));
                    }
                }
                s.album.insert(id, bytes);
            }
        }
        s.largest.sort_by_key(|&(_, _, bytes)| std::cmp::Reverse(bytes));
        let ids: Vec<PathBuf> = app.tracks.iter().map(Track::id).collect();
        let by_path = ids.iter().map(PathBuf::as_path).zip(&app.tracks).collect();
        let planned = crate::card::playlist::planned(&w.chosen_playlists(&app.saved.playlists), &by_path, &w.playlists_folder, rules);
        s.playlists = planned.iter().flat_map(|(_, copies, _)| copies.iter().map(|c| card_size(c.track, c.target))).sum();
        s.songs += planned.iter().map(|(_, copies, _)| copies.len()).sum::<usize>();
        s
    }
}

fn capacity(app: &mut App, ui: &mut Ui, sizes: &Sizes) {
    let ink = ink(ui);
    let w = app.write();
    let need = sizes.selected + sizes.playlists;
    let space = room(app);
    row_wrapped(ui, |ui| {
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
    let device = w.options.device;
    if let Some(max) = device.max_songs()
        && sizes.songs > max
    {
        let copies = if sizes.playlists > 0 { ", playlist copies included" } else { "" };
        ui.label(RichText::new(format!("{} chosen{copies}, but {} lists at most {}: the rest won't show up on the player.", plural(sizes.songs, "song"), device.name(), text::count(max))).color(ink.warn).size(13.0));
    }
    let Some((_, room)) = space else { return };
    let adoptable = app.card_survey.as_ref().filter(|(n, _)| *n == w.id).map(|(_, s)| s.used_bytes()).unwrap_or(0);
    if adoptable > 0 {
        muted(ui, format!("counting the {} a take-over moves into place as already there", text::bytes(adoptable)));
    }
    space_bar(ui, need, room, ui.available_width().min(560.0));
    if need > room {
        fit(app, ui, sizes, need - room);
    }
}

/// Ways to make the choice fit, by how much each saves: smaller files first (nothing to
/// choose again), then the artists taking most room.
fn fit(app: &mut App, ui: &mut Ui, sizes: &Sizes, over: u64) {
    let ink = ink(ui);
    ui.label(RichText::new(format!("doesn't fit: {} over. Some ways to make it fit:", text::bytes(over))).color(ink.warn).size(13.0));
    let enough = |saving: u64| if saving >= over { ", enough to fit" } else { "" };
    let mut set = None;
    if sizes.cd_saving > 0 {
        row_wrapped(ui, |ui| {
            muted(ui, format!("Write hi-res FLAC at CD quality: saves about {}{}.", text::bytes(sizes.cd_saving), enough(sizes.cd_saving)));
            if widgets::icon_link(ui, Icon::Check, "do that").on_hover_text("16-bit, 44.1 or 48 kHz FLAC for songs above that; through earbuds on a portable player the difference isn't audible").clicked() {
                set = Some(false);
            }
        });
    }
    if sizes.mp3_saving > 0 {
        row_wrapped(ui, |ui| {
            muted(ui, format!("Write lossless songs as MP3 320: saves about {}{}.", text::bytes(sizes.mp3_saving), enough(sizes.mp3_saving)));
            if widgets::icon_link(ui, Icon::Check, "do that").on_hover_text("Hard to tell apart from the original on a portable player; your music folder keeps the lossless files, so this can be undone at any sync").clicked() {
                set = Some(true);
            }
        });
    }
    match set {
        Some(true) => (app.write_mut().options.cd_quality, app.write_mut().options.lossless_mp3) = (true, true),
        Some(false) => app.write_mut().options.cd_quality = true,
        None => {}
    }
    if set.is_some() {
        app.refresh_on_card();
        return;
    }
    // Enough of the largest artists to cover what's over, but never a long list (this panel
    // stays in view).
    let mut shown = Vec::new();
    let mut covered = 0;
    for &(ai, albums, bytes) in sizes.largest.iter().take(5) {
        if covered >= over {
            break;
        }
        covered += bytes;
        shown.push((ai, albums, bytes));
    }
    if shown.is_empty() {
        return;
    }
    muted(ui, if covered >= over { "Or leave out the artists taking most room; these would be enough:" } else { "Or leave out the artists taking most room:" });
    let mut leave = None;
    for (ai, albums, bytes) in shown {
        let a = &app.index.artists[ai];
        row(ui, |ui| {
            ui.add_space(S2);
            widgets::fitted(ui, RichText::new(&a.name).color(ink.text), 280.0, Sense::hover()).on_hover_text(&a.name);
            muted(ui, format!("{} · {}", plural(albums, "album"), text::bytes(bytes)));
            if widgets::icon_link(ui, Icon::Remove, "leave out").on_hover_text("or untick some of their albums below").clicked() {
                leave = Some(ai);
            }
        });
    }
    if let Some(ai) = leave {
        app.card_artist(ai, false);
    }
}

/// The last comparison with the card, and what else is on it.
fn comparison(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    if let Some((_, p)) = &app.write_preview {
        let converting = if p.to_convert > 0 { format!(", {} of them converted or cut", p.to_convert) } else { String::new() };
        let mut parts = vec![format!("{} to copy ({}{converting})", p.to_copy, text::bytes(p.copy_bytes)), format!("{} already there", p.unchanged)];
        if p.renamed > 0 {
            parts.push(format!("{} to rename there", p.renamed));
        }
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

/// Music already on the card that this write didn't put there: one line here, and the
/// dialog that goes through it.
fn take_over(app: &mut App, ui: &mut Ui) {
    let Some((_, p)) = &app.write_preview else { return };
    let (foreign, foreign_bytes, can_undo) = (p.foreign, p.foreign_bytes, p.can_undo_take_over);
    if foreign == 0 && !can_undo {
        return;
    }
    row_wrapped(ui, |ui| {
        if foreign > 0 {
            muted(ui, format!("The card also has {} this write didn't put there ({}).", plural(foreign, "file"), text::bytes(foreign_bytes)));
            if widgets::icon_link(ui, Icon::Card, "go through them").on_hover_text("songs you copied by hand can be moved into this layout instead of copied again").clicked() {
                app.take_over_open = true;
            }
        }
        if can_undo && app.busy.is_none() && widgets::icon_link(ui, Icon::Back, "undo taking over the card").on_hover_text("move the files back to where they were").clicked() {
            app.undo_take_over();
        }
    });
}

/// Taking over a card's own music, step by step: check it against the library, then move the
/// copies of library songs into place.
pub fn take_over_dialog(app: &mut App, ui: &mut Ui) {
    if !app.take_over_open {
        return;
    }
    // The comparison it rests on went stale (options changed, another write chosen).
    if app.write_preview.is_none() && app.card_survey.is_none() && app.busy.is_none() {
        app.take_over_open = false;
        return;
    }
    let ink = ink(ui);
    let frame = super::theme::dialog_frame(ui);
    let r = egui::Modal::new(egui::Id::unique("take over")).frame(frame).show(ui.ctx(), |ui| {
        ui.set_width(640.0f32.min(ui.ctx().content_rect().width() - 80.0));
        row(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            super::icons::paint(ui.painter(), rect, Icon::Card, ink.text);
            ui.label(RichText::new("Music already on the card").size(18.0).color(ink.text));
        });
        ui.add_space(S2);
        if let Some(b) = &app.busy {
            row(ui, |ui| {
                super::cassette::spinner(ui, 14.0);
                muted(ui, format!("{}…", b.label));
            });
            return;
        }
        take_over_steps(app, ui);
    });
    if r.should_close() {
        app.take_over_open = false;
    }
}

fn take_over_steps(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let id = &app.write().id;
    let (foreign, foreign_bytes) = app.write_preview.as_ref().map_or((0, 0), |(_, p)| (p.foreign, p.foreign_bytes));
    // Counts and sizes, copied out so the buttons below can change the app.
    let surveyed = app.card_survey.as_ref().filter(|(n, _)| n == id).map(|(_, s)| {
        let c = |l: &[(PathBuf, u64)]| (l.len(), Survey::bytes(l));
        ((s.used.len(), s.used_bytes()), c(&s.spare), c(&s.versions), c(&s.unknown), c(&s.other))
    });
    let clutter = app.card_survey.as_ref().filter(|(n, _)| n == id).map(|(_, s)| s.clutter()).unwrap_or_default();
    match surveyed {
        None if foreign > 0 => {
            ui.label(RichText::new(format!("The card has {} that this write didn't put there ({}): music copied by hand, perhaps.", plural(foreign, "file"), text::bytes(foreign_bytes))).color(ink.text));
            ui.add_space(S1);
            for (n, step) in [
                "Check them against your library. This reads a little of each file and changes nothing.",
                "See what they are: exact copies of songs this write needs, other versions, music your library doesn't have, and other files.",
                "Take over the card: the copies are moved into this write's layout (renamed on the card, instantly) instead of copied again. Nothing else is deleted unless you say so, and undo puts everything back.",
            ]
            .iter()
            .enumerate()
            {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(format!("{}.", n + 1)).color(ink.muted));
                    ui.add(egui::Label::new(RichText::new(*step).color(if n == 0 { ink.text } else { ink.muted })).wrap());
                });
            }
            ui.add_space(S2);
            row(ui, |ui| {
                if app.busy.is_none() && widgets::primary_with(ui, Icon::Compare, "Check them against the library").on_hover_text("reads a little of each file; changes nothing").clicked() {
                    app.survey_card();
                }
                if widgets::button(ui, "Not now").on_hover_text("the card tab offers it again").clicked() {
                    app.take_over_open = false;
                }
            });
        }
        None => {}
        Some(((used, used_bytes), (spare, spare_bytes), (versions, versions_bytes), (unknown, unknown_bytes), (other, other_bytes))) => {
            // Albums the card has (some songs of, at least) that this write doesn't choose: one
            // click chooses them, so the write starts from what the card holds.
            let found: Vec<(usize, usize)> = app
                .index
                .artists
                .iter()
                .enumerate()
                .flat_map(|(ai, a)| a.albums.iter().enumerate().map(move |(bi, al)| (ai, bi, al)))
                .filter(|(_, _, al)| al.tracks.iter().any(|t| app.card_state.by_hand.contains(t)) && !al.tracks.iter().any(|t| app.card_state.planned.contains(t)))
                .map(|(ai, bi, _)| (ai, bi))
                .collect();
            let to_tick = !found.is_empty();
            if to_tick {
                ui.label(RichText::new(format!("{} from your library {} on the card, not ticked in this write.", plural(found.len(), "album"), if found.len() == 1 { "is" } else { "are" })).color(ink.text));
                muted(ui, "Tick them, and this write starts from what the card holds: taking the card over then moves their songs into place instead of copying them again.");
                ui.add_space(S1);
                row(ui, |ui| {
                    if app.busy.is_none() && widgets::primary_with(ui, Icon::Check, "Tick them").clicked() {
                        for (ai, bi) in found {
                            app.card_album(ai, bi, true);
                        }
                        app.survey_card();
                    }
                });
                ui.add_space(S1);
            }
            if used > 0 || !to_tick {
                let what = if used == 1 { "is an exact copy of a song" } else { "are exact copies of songs" };
                ui.label(RichText::new(format!("{} on the card {what} this write needs ({}).", plural(used, "song"), text::bytes(used_bytes))).color(ink.text));
                muted(ui, "They're moved into place (renamed on the card, instant) instead of copied again.");
            }
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
                row_wrapped(ui, |ui| {
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
                        ui.label(RichText::new("not enough room in your music folder's drive to copy them").size(12.5).color(ink.warn));
                    }
                });
            }
            if other > 0 {
                muted(ui, format!("{} of other kinds: covers, logs, playlists, other devices' data… ({}).", plural(other, "file"), text::bytes(other_bytes)));
            }
            ui.add_space(S1);
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
            ui.add_space(S1);
            row(ui, |ui| {
                // With albums still to tick, ticking them is the step to take first.
                let take_over = |ui: &mut Ui| if to_tick { widgets::button_with(ui, Icon::Card, "Take over the card") } else { widgets::primary_with(ui, Icon::Card, "Take over the card") };
                if app.busy.is_none() && used + spare + versions + unknown + other > 0 && take_over(ui).on_hover_text("only moves files on the card; undo puts everything back").clicked() {
                    app.take_over_card();
                }
                if widgets::button(ui, "Not now").on_hover_text("the card tab offers it again").clicked() {
                    app.card_survey = None;
                    app.take_over_open = false;
                }
            });
        }
    }
}

fn layout_options(app: &mut App, ui: &mut Ui) {
    let device = app.player();
    let unplayable = crate::library::unplayable_summary(&app.tracks, |t| device.plays(t));
    let mut changed = false;
    let ink = ink(ui);
    let w = &mut app.saved.writes[app.saved.active_write];
    widgets::section(ui, Icon::Settings, "Folders and options");
    ui.add_space(S1);
    widgets::setting(ui, "Album folders", |ui| {
        row(ui, |ui| {
            for (order, label) in [(AlbumOrder::Year, "1997 - Album"), (AlbumOrder::Title, "Album"), (AlbumOrder::Custom, "01 - Album")] {
                // Each is a pattern of names; choosing one sets it back to that.
                if ui.radio(w.options.pattern.is_none() && w.options.album_order == order, label).clicked() {
                    w.options.album_order = order;
                    w.options.pattern = None;
                    app.pattern_draft = None;
                    changed = true;
                }
            }
        });
        widgets::help(ui, "By year (oldest first), by title, or numbered in your own order: use the arrows in the list above.");
    });
    widgets::setting(ui, "Names on the card", |ui| {
        let mut text = app.pattern_draft.clone().unwrap_or_else(|| w.options.pattern_text().to_string());
        let r = ui.add(egui::TextEdit::singleline(&mut text).align(egui::Align2::LEFT_CENTER).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
        let parsed = Pattern::parse(&text);
        if r.changed() {
            if parsed.is_ok() {
                let own = text.trim() != default_for(w.options.album_order);
                w.options.pattern = own.then(|| text.trim().to_string());
                changed = true;
            }
            app.pattern_draft = Some(text.clone());
        }
        match &parsed {
            Err(why) => {
                ui.label(RichText::new(format!("{why}; the names stay as they were")).size(13.0).color(ink.warn));
            }
            Ok(p) => {
                if let Some(e) = pattern_example(&app.index, &app.tracks, w, p) {
                    widgets::help(ui, format!("e.g. {e}"));
                }
            }
        }
        row_wrapped(ui, |ui| {
            muted(ui, "fields:");
            for f in Field::ALL {
                ui.label(RichText::new(format!("{{{}}}", f.name())).monospace().size(12.5).color(ink.text)).on_hover_text(f.about());
            }
        });
        widgets::help(ui, "/ makes a folder. A [part] is left out when a field in it is empty. Songs already on the card are renamed there at the next sync, not copied again.");
        if w.options.pattern.is_some() && widgets::icon_link(ui, Icon::Back, "back to the album folders above").clicked() {
            w.options.pattern = None;
            app.pattern_draft = None;
            changed = true;
        }
    });
    widgets::setting(ui, "Other folders", |ui| {
        egui::Grid::new("folders").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            muted(ui, "songs without an album");
            changed |= ui.add(egui::TextEdit::singleline(&mut w.options.singles_folder).align(egui::Align2::LEFT_CENTER).desired_width(160.0)).changed();
            ui.end_row();
            muted(ui, "playlists");
            changed |= ui.add(egui::TextEdit::singleline(&mut w.playlists_folder).align(egui::Align2::LEFT_CENTER).desired_width(160.0)).changed();
            ui.end_row();
        });
    });
    if let Some((n, formats)) = &unplayable {
        widgets::setting(ui, &format!("Formats {} can't play", device.short()), |ui| {
            ui.label(RichText::new(format!("{}: {formats}", plural(*n, "song"))).color(ink.text));
            widgets::help(
                ui,
                format!(
                    "{} In this app, AAC, ALAC, AIFF and Ogg play as they are; {}; Dolby AC-4 has no decoder yet.",
                    device.plays_text(),
                    if crate::audio::ffmpeg().is_some() {
                        "Dolby (E-AC-3, AC-3), Opus, APE, WavPack and WMA through ffmpeg, in stereo".to_string()
                    } else {
                        format!("Dolby (E-AC-3, AC-3), Opus, APE, WavPack and WMA need ffmpeg ({})", crate::platform::FFMPEG_INSTALL)
                    }
                ),
            );
            changed |= ui
                .radio_value(&mut w.options.unplayable, Unplayable::Convert, "convert them")
                .on_hover_text("Takes a while the first time; later writes keep what's already on the card.")
                .changed();
            if w.options.unplayable == Unplayable::Convert {
                // What converting would make of them, and what still can't go (no ffmpeg, AC-4…).
                let (mut flac, mut mp3, mut left) = (0, 0, BTreeMap::<&str, usize>::new());
                let rules = Rules { unplayable: Unplayable::Convert, ..w.options.rules() };
                for t in app.tracks.iter().filter(|t| !device.plays(t) && t.cue.is_none()) {
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
                    let which = if rules.lossless_mp3 { "in stereo, as chosen under saving space" } else { "AAC, Dolby and other lossy ones, in stereo" };
                    made.push(format!("{} to MP3 320 ({which})", plural(mp3, "song")));
                }
                row(ui, |ui| {
                    ui.add_space(S3);
                    ui.vertical(|ui| {
                        for m in made {
                            widgets::help(ui, m);
                        }
                        for (why, n) in left {
                            ui.label(RichText::new(format!("{} left out: {why}.", plural(n, "song"))).color(ink.warn).size(13.0));
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
                            "{} will be copied but won't play on {}. {} They'll take {} of space and show up as files that do nothing.",
                            plural(*n, "file"),
                            device.short(),
                            device.plays_text(),
                            text::bytes(app.tracks.iter().filter(|t| !device.plays(t)).map(|t| t.size).sum())
                        ))
                        .color(ink.warn),
                    );
                });
            }
        });
    }
    widgets::setting(ui, "Saving space", |ui| {
        let mut quality = (w.options.cd_quality || w.options.lossless_mp3, w.options.lossless_mp3);
        let before = quality;
        ui.radio_value(&mut quality, (false, false), "lossless songs as they are");
        ui.radio_value(&mut quality, (true, false), "hi-res songs at CD quality");
        widgets::help(ui, "16-bit, 44.1 or 48 kHz FLAC instead of 24-bit at 96 or 192 kHz: a third to a sixth of the space. Through earbuds the difference isn't audible.");
        ui.radio_value(&mut quality, (true, true), "all lossless songs as MP3 320");
        widgets::help(ui, "About a quarter of the space of CD-quality FLAC. Your music folder keeps the originals either way, and choosing again re-makes the card's copies at the next sync.");
        if quality != before {
            (w.options.cd_quality, w.options.lossless_mp3) = quality;
            changed = true;
        }
    });
    widgets::setting(ui, "Housekeeping", |ui| {
        changed |= ui.checkbox(&mut w.options.write_tags, "write corrected tags into the copies").changed();
        changed |= ui.checkbox(&mut w.prune, "remove what this write put on the card that's no longer chosen").changed();
        changed |= ui.checkbox(&mut w.options.clean_mac_files, "delete macOS ._ and .DS_Store files on the card").changed();
    });
    if changed {
        // Names and sizes on the card may have changed with the options.
        app.write_preview = None;
        app.refresh_on_card();
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
    row_wrapped(ui, |ui| {
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
    widgets::help(ui, "A tick means it's on the card after the next Sync; the mark beside it says what Sync does to get there. On the right, the folder names on the card; rename changes them for this write only.");
    ui.add_space(S1);
    legend(ui, !app.card_state.by_hand.is_empty());
    ui.add_space(S1);
    let changes = app.card_state.added + app.card_state.replaced + app.card_state.removed;
    if changes == 0 {
        app.card_changes_only = false;
    }
    row(ui, |ui| {
        if widgets::tab(ui, "everything", !app.card_changes_only).clicked() {
            app.card_changes_only = false;
        }
        ui.add_space(S2);
        let label = format!("what the next Sync changes ({changes})");
        if changes > 0 && widgets::tab(ui, &label, app.card_changes_only).on_hover_text("Only the albums Sync adds or takes off, to look over before it runs.").clicked() {
            app.card_changes_only = true;
        } else if changes == 0 {
            muted(ui, "no album changes at the next Sync");
        }
    });
    ui.add_space(S1);
    row(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        super::icons::paint(ui.painter(), rect, Icon::Search, ink.muted);
        let r = widgets::search_field(ui, &mut app.card_filter, "filter", 200.0);
        if app.focus_search && app.tab == crate::app::Tab::Card {
            app.focus_search = false;
            r.request_focus();
        }
        if widgets::icon_link(ui, Icon::Add, "tick all").clicked() {
            edits.push(Edit::All(true));
        }
        if widgets::icon_link(ui, Icon::Remove, "untick all").clicked() {
            edits.push(Edit::All(false));
        }
    });
    ui.add_space(S1);
    let w = app.write();
    let needle = fold(&app.card_filter);
    let pattern = w.options.pattern();
    let custom_order = pattern.uses(Field::Position);
    for (ai, artist) in app.index.artists.iter().enumerate() {
        let ck = &app.index.clusters[artist.cluster].key;
        if !needle.is_empty()
            && !fold(&artist.name).contains(&needle)
            && !artist.albums.iter().any(|a| a.title.as_deref().is_some_and(|t| fold(t).contains(&needle)))
        {
            continue;
        }
        // Where each album stands with the card: the mark beside its tick, and in the artist's row
        // how many are where.
        let status: Vec<(Option<Mark>, String)> = artist.albums.iter().map(|al| album_status(app, ck, artist, al)).collect();
        let changing = |m: &Option<Mark>| matches!(m, Some(Mark::Adding | Mark::Removing));
        if app.card_changes_only && !status.iter().any(|(m, _)| changing(m)) {
            continue;
        }
        // Ticked when everything goes, half when some does.
        let chosen: Vec<bool> = artist.albums.iter().map(|al| w.custom.includes(ck, al, &artist.albums)).collect();
        let (any, all) = (chosen.iter().any(|&c| c), chosen.iter().all(|&c| c));
        let whole = w.custom.whole_artist(ck);
        let card_name = w.custom.artist_names.get(ck).cloned().unwrap_or_else(|| artist.name.clone());
        let open = app.card_changes_only || app.card_expanded.contains(ck);
        row(ui, |ui| {
            let mut on = all && any;
            let hint = if whole { "the whole artist goes on the card, albums added later too" } else { "tick to put the whole artist on the card, albums added later too" };
            if ui.add(egui::Checkbox::without_text(&mut on).indeterminate(any && !all)).on_hover_text(hint).changed() {
                edits.push(Edit::Artist(ai, on));
            }
            let name = RichText::new(format!("{} {}", if open { "▾" } else { "▸" }, artist.name)).size(16.0).color(if any { ink.text } else { ink.muted });
            // Leave room for the size and the "rename" link on the right.
            let r = widgets::fitted(ui, name, ui.available_width() - 200.0, Sense::click()).on_hover_text(&artist.name);
            widgets::focus_ring(ui, &r, r.rect, 2.0);
            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                edits.push(Edit::Expand(ck.clone()));
            }
            muted(ui, text::bytes(sizes.artist.get(ck).copied().unwrap_or(0)));
            for (kind, words) in [(Mark::On, "on card"), (Mark::ByHand, "copied by hand"), (Mark::Adding, "added"), (Mark::Removing, "taken off")] {
                let n = status.iter().filter(|(m, _)| *m == Some(kind)).count();
                if n > 0 {
                    ui.add_space(S1);
                    mark_words(ui, kind, &format!("{n} {words}"));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Clear of the scrollbar.
                ui.add_space(S2);
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
            // Where the album's first song goes: its folders, as the pattern makes them.
            let folder = al.tracks.first().map(|&i| {
                let t = &app.tracks[i];
                let rel = song_path(&pattern, t, al, &card_name, title, positions[n], &w.options, t.ext.as_str());
                rel.parent().map(|d| d.iter().map(|c| c.to_string_lossy()).collect::<Vec<_>>().join(" / ")).unwrap_or_default()
            });
            let mut on = w.custom.includes(ck, al, &artist.albums);
            let on_card = on;
            let bi = artist.albums.iter().position(|a| std::ptr::eq(a, *al)).unwrap_or_default();
            let (mark, said) = &status[bi];
            if app.card_changes_only && !changing(mark) {
                continue;
            }
            row(ui, |ui| {
                ui.add_space(S3);
                let hint = if al.edition.is_some() { "One edition of this album goes on the card; ticking this one replaces the other." } else { "" };
                let r = ui.add(egui::Checkbox::without_text(&mut on));
                if (if hint.is_empty() { r } else { r.on_hover_text(hint) }).changed() {
                    edits.push(Edit::Album(ai, bi, on));
                }
                let shown = al.title.as_deref().unwrap_or("Loose tracks");
                let mut title_text = RichText::new(shown).italics().color(if on_card { ink.text } else { ink.muted });
                if *mark == Some(Mark::Removing) {
                    title_text = title_text.strikethrough();
                }
                let title_w = ui.available_width() * if al.edition.is_some() { 0.25 } else { 0.4 };
                widgets::fitted(ui, title_text, title_w, Sense::hover()).on_hover_text(shown);
                if let Some(m) = mark {
                    mark_words(ui, *m, said);
                }
                if let Some(e) = &al.edition {
                    let mut hover = format!("edition {} of {}: {} · {}", e.rank + 1, e.count, e.label, e.format);
                    if let Some(s) = &e.superseded {
                        hover.push_str(&format!("\nEdition {} has all these songs in {} quality.", s.rank + 1, if s.higher { "higher" } else { "the same" }));
                    }
                    ui.label(RichText::new(format!("{} · {}", text::shorten(&e.label, 30), e.format)).size(12.5).color(ink.muted)).on_hover_text(hover);
                }
                muted(ui, format!("{} · {}", plural(al.tracks.len(), "track"), text::bytes(sizes.album.get(id).copied().unwrap_or(0))));
                // Songs the player can't read as they are: converted, copied anyway, or left out.
                let device = w.options.device;
                let fates: Vec<Result<Target, &str>> = al.tracks.iter().map(|&i| &app.tracks[i]).filter(|t| !device.plays(t) && t.cue.is_none()).map(|t| target(t, w.options.rules())).collect();
                let converted = fates.iter().filter(|f| matches!(f, Ok(Target::Flac | Target::FlacCd | Target::Mp3))).count();
                let copied = fates.iter().filter(|f| matches!(f, Ok(Target::Copy))).count();
                let left: Vec<&str> = al.tracks.iter().filter_map(|&i| target(&app.tracks[i], w.options.rules()).err()).collect();
                if converted > 0 {
                    muted(ui, format!("{converted} converted")).on_hover_text(format!("{} These are converted on the way (lossless to FLAC, the rest to MP3 320).", device.plays_text()));
                }
                if copied > 0 {
                    ui.label(RichText::new(format!("{copied} won't play")).size(12.5).color(ink.warn)).on_hover_text(format!("copied as they are, though they won't play on {}; change this under options below", device.short()));
                }
                if let Some(why) = left.first() {
                    ui.label(RichText::new(format!("{} left out", left.len())).size(12.5).color(ink.warn)).on_hover_text(*why);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(S2);
                    if custom_order && al.title.is_some() {
                        if widgets::icon_button(ui, Icon::Down, "move down").clicked() {
                            edits.push(Edit::Move(ck.clone(), order_ids.clone(), n, 1));
                        }
                        if widgets::icon_button(ui, Icon::Up, "move up").clicked() {
                            edits.push(Edit::Move(ck.clone(), order_ids.clone(), n, -1));
                        }
                    }
                    if al.title.is_some() {
                        rename_widget(ui, &app.card_renaming, false, id, title.unwrap_or(al.title.as_deref().unwrap_or_default()), &mut edits);
                    }
                    let path = folder.clone().unwrap_or_default();
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
        match e {
            Edit::Expand(k) => {
                if !app.card_expanded.remove(&k) {
                    app.card_expanded.insert(k);
                }
            }
            Edit::StartRename(is_artist, key, current) => app.card_renaming = Some((is_artist, key, current)),
            Edit::Rename(is_artist, key, name) => {
                app.remember_chosen(if name.trim().is_empty() { "going back to the library's name on the card".to_string() } else { format!("renaming to “{}” on the card", name.trim()) });
                let w = &mut app.saved.writes[app.saved.active_write];
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
                app.remember_chosen(if on { "ticking everything" } else { "unticking everything" });
                let w = &mut app.saved.writes[app.saved.active_write];
                for a in &app.index.artists {
                    w.custom.set_artist(&app.index.clusters[a.cluster].key, &a.albums, on);
                }
                touched_layout = true;
            }
            Edit::Move(ck, mut ids, n, d) => {
                let m = n as isize + d;
                if m >= 0 && (m as usize) < ids.len() {
                    app.remember_chosen("changing the order of albums");
                    let w = &mut app.saved.writes[app.saved.active_write];
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

/// What the marks mean, as in the library: there already, coming, going.
fn legend(ui: &mut Ui, by_hand: bool) {
    row_wrapped(ui, |ui| {
        mark_words(ui, Mark::On, "on the card");
        ui.add_space(S2);
        if by_hand {
            mark_words(ui, Mark::ByHand, "on the card, copied by hand");
            ui.add_space(S2);
        }
        mark_words(ui, Mark::Adding, "added at the next Sync");
        ui.add_space(S2);
        mark_words(ui, Mark::Removing, "taken off at the next Sync");
    });
}

/// A card mark and its words, coloured by what it says: muted for what stays, ink for what's
/// coming, the warning colour for what goes.
fn mark_words(ui: &mut Ui, m: Mark, words: &str) {
    let ink = ink(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    m.paint(ui.painter(), rect.center(), 3.5, &ink);
    let color = match m {
        Mark::On => ink.muted,
        Mark::Adding | Mark::ByHand => ink.text,
        Mark::Removing => ink.warn,
    };
    ui.label(RichText::new(words).size(13.0).color(color));
}

/// "rename" link, or the inline editor when this row is being renamed.
fn rename_widget(ui: &mut Ui, renaming: &Option<(bool, String, String)>, is_artist: bool, key: &str, current: &str, edits: &mut Vec<Edit>) {
    match renaming {
        Some((a, k, buf)) if *a == is_artist && k == key => {
            let mut text = buf.clone();
            if widgets::icon_link(ui, Icon::Check, "save").clicked() {
                edits.push(Edit::Rename(is_artist, key.to_string(), text.clone()));
            }
            let r = ui.add(egui::TextEdit::singleline(&mut text).align(egui::Align2::LEFT_CENTER).desired_width(200.0));
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
