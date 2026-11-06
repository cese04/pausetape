//! The player: cover art, title, a loudness sparkline that doubles as the scrubber, and the
//! transport buttons. Following Tufte's sparklines, the only colour is a small red mark at the
//! playhead.

use std::time::Duration;

use egui::{Align, Align2, FontId, Layout, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};

use super::text;
use super::theme::ink;
use super::widgets::{Media, media_button, muted};
use crate::app::App;
use crate::audio::Status;
use super::layout::{ROW, S1, S2, S3, S4, S5, S6, row, font};

/// Every transport button sits in a slot this tall, so different sizes share one centre line.
const SLOT: f32 = S6;
/// The sparkline's height.
const WAVE: f32 = S4;
/// Side of the cover square: the title, the sparkline and the buttons beside it, with their gaps.
const COVER: f32 = ROW + S1 + WAVE + S1 + SLOT;
/// The compact player's height, and its cover's side.
const MINI: f32 = S5;
/// A window lower than this always has the compact player, so the page keeps most of it.
const LOW: f32 = 640.0;

/// One line rather than the full player: asked for (a click on the cover), or the window is low.
pub fn compact(app: &App, ctx: &egui::Context) -> bool {
    app.saved.compact_player || ctx.content_rect().height() < LOW
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let st = app.audio.status();
    if let Some(e) = &st.error {
        ui.label(RichText::new(e).color(ink.warn).size(font::META));
    }
    let Some(ti) = app.current() else {
        muted(ui, "Nothing playing. Double-click a song in the library.");
        return;
    };
    let len = st.len.filter(|d| !d.is_zero()).unwrap_or(app.tracks[ti].duration);
    if st.path.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(200));
    }

    if compact(app, ui.ctx()) {
        compact_row(app, ui, ti, &st, len);
    } else {
        ui.horizontal_top(|ui| {
            cover_art(app, ui, ti, COVER);
            ui.add_space(S1);
            ui.vertical(|ui| {
                title_row(app, ui, ti, &st, len);
                scrubber(app, ui, ti, &st, len);
                transport(app, ui, &st, len);
            });
        });
    }
    keyboard(app, ui, &st, len);
}

/// The player on one line: cover, title and artist, the buttons, the sparkline, the time, the
/// queue and the volume.
fn compact_row(app: &mut App, ui: &mut Ui, ti: usize, st: &Status, len: Duration) {
    let ink = ink(ui);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), MINI), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = S2;
        cover_art(app, ui, ti, MINI);
        let t = &app.tracks[ti];
        let words_w = (ui.available_width() * 0.25).clamp(100.0, 280.0);
        ui.allocate_ui_with_layout(vec2(words_w, MINI), Layout::top_down(Align::Min), |ui| {
            ui.set_width(words_w);
            ui.spacing_mut().item_spacing.y = 0.0;
            super::widgets::fitted(ui, RichText::new(t.display_title()).size(font::BODY).color(ink.text), words_w, Sense::hover());
            let meta: Vec<String> = [t.artist.clone(), t.album.clone()].into_iter().flatten().collect();
            super::widgets::fitted(ui, RichText::new(meta.join(" · ")).size(font::UI).color(ink.muted), words_w, Sense::hover());
        });
        buttons(app, ui, st, S3, S4);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            volume_control(app, ui, false);
            queue_button(app, ui, st, len, true);
            ui.label(RichText::new(format!("{} / {}", text::duration(st.pos), text::duration(len))).font(super::widgets::data()).color(ink.muted));
            scrubber(app, ui, ti, st, len);
        });
    });
}

fn cover_art(app: &mut App, ui: &mut Ui, ti: usize, side: f32) {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    // A click on the cover folds the player to one line, or opens it out again.
    let forced = ui.ctx().content_rect().height() < LOW;
    let hint = match (app.saved.compact_player, forced) {
        (_, true) => "The window is too low for the full player",
        (false, _) => "Fold the player to one line",
        (true, _) => "Open the player out",
    };
    super::widgets::name(ui, &resp, egui::Role::Button, hint);
    super::widgets::focus_ring(ui, &resp, rect, 3.0);
    if !forced && resp.clicked() {
        app.saved.compact_player = !app.saved.compact_player;
    }
    resp.on_hover_text(hint).on_hover_cursor(if forced { egui::CursorIcon::Default } else { egui::CursorIcon::PointingHand });
    let t = &app.tracks[ti];
    match app.cover.as_ref().filter(|(p, _, _)| *p == t.path).and_then(|(_, tex, _)| tex.as_ref()) {
        Some(tex) => {
            // Fit inside the square without cropping; most covers are square anyway.
            let size = tex.size_vec2();
            let scale = (side / size.x).min(side / size.y);
            let r = egui::Rect::from_center_size(rect.center(), size * scale);
            egui::Image::new(tex).corner_radius(3.0).paint_at(ui, r);
        }
        None => {
            // No art: a quiet square with the album's initial, in the manner of a library card.
            ui.painter().rect(rect, 3.0, ink.hover, Stroke::new(1.0, ink.faint), egui::StrokeKind::Inside);
            let initial = t.album.as_deref().or(t.title.as_deref()).and_then(|s| s.chars().next()).unwrap_or('♪');
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, initial, FontId::proportional(side * 0.33), ink.faint);
        }
    }
}

/// Title, then artist · album · year, then a note on the format; the time at the right end.
/// The time keeps its place: a long title, then the rest, shorten with "…" (whole on hover), so
/// nothing is pushed past the window's edge.
fn title_row(app: &App, ui: &mut Ui, ti: usize, st: &Status, len: Duration) {
    let ink = ink(ui);
    let t = &app.tracks[ti];
    let fit = super::widgets::fitted;
    row(ui, |ui| {
        let time = format!("{} / {}", text::duration(st.pos), text::duration(len));
        let note = if st.converting {
            Some((RichText::new("converting with ffmpeg…").size(font::META).color(ink.muted), None))
        } else {
            text::format_note(t, app.player()).map(|n| (RichText::new(text::format_badge(t, app.player())).size(font::META).color(ink.warn), Some(n)))
        };
        let note_w = note.as_ref().map(|(r, _)| super::widgets::text_width(ui, r.text(), font::META) + S2).unwrap_or(0.0);
        let room = ui.available_width() - super::widgets::data_width(ui, &time) - S3 - note_w;
        let title = t.display_title();
        let meta: Vec<String> = [t.artist.clone(), t.album.clone(), t.year.map(|y| y.to_string())].into_iter().flatten().collect();
        let meta = meta.join(" · ");
        // The title first, but leaving the rest a little room when there's some to leave.
        let meta_w = super::widgets::text_width(ui, &meta, font::BODY);
        let title_w = (room - meta_w.min(room * 0.35)).max(room * 0.5);
        let shown = fit(ui, RichText::new(&title).size(font::TITLE).color(ink.text), title_w, Sense::hover()).on_hover_text(&title);
        let rest = room - shown.rect.width() - ui.spacing().item_spacing.x;
        if rest > 40.0 {
            fit(ui, RichText::new(&meta).color(ink.muted), rest, Sense::hover()).on_hover_text(&meta);
        }
        if let Some((r, hint)) = note {
            let r = ui.label(r);
            if let Some(h) = hint {
                r.on_hover_text(h);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(time).font(super::widgets::data()).color(ink.muted));
        });
    });
}

fn scrubber(app: &mut App, ui: &mut Ui, ti: usize, st: &Status, len: Duration) {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), WAVE), Sense::click_and_drag());
    let p = ui.painter_at(rect.expand(4.0));
    let frac = if len.is_zero() { 0.0 } else { (st.pos.as_secs_f32() / len.as_secs_f32()).clamp(0.0, 1.0) };
    let mid = rect.center().y;
    let playhead_x = rect.left() + rect.width() * frac;
    match app.envelope.as_ref().filter(|(id, _)| *id == app.tracks[ti].id()) {
        Some((_, env)) if !env.is_empty() => {
            let cols = (rect.width() / 2.0).max(1.0) as usize;
            let level = |c: usize| {
                let a = c * env.len() / cols;
                let b = ((c + 1) * env.len() / cols).max(a + 1).min(env.len());
                env[a..b].iter().cloned().fold(0.0f32, f32::max)
            };
            for c in 0..cols {
                let x = rect.left() + c as f32 * 2.0 + 0.5;
                let h = (level(c) * rect.height() * 0.5).max(0.5);
                let color = if x <= playhead_x { ink.text } else { ink.faint };
                p.add(Shape::line_segment([pos2(x, mid - h), pos2(x, mid + h)], Stroke::new(1.0, color)));
            }
        }
        _ => {
            p.hline(rect.x_range(), mid, Stroke::new(1.0, ink.faint));
            p.hline(rect.left()..=playhead_x, mid, Stroke::new(1.5, ink.accent));
        }
    }
    p.circle_filled(pos2(playhead_x, mid), 2.5, ink.accent);
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.clicked() || resp.drag_stopped())
    {
        let f = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        app.audio.seek(len.mul_f32(f));
    }
    if let Some(pos) = resp.hover_pos() {
        let f = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        resp.on_hover_text_at_pointer(text::duration(len.mul_f32(f)));
    }
}

fn transport(app: &mut App, ui: &mut Ui, st: &Status, len: Duration) {
    row(ui, |ui| {
        ui.spacing_mut().item_spacing.x = S2;
        buttons(app, ui, st, S4, S5);
        ui.add_space(S2);
        queue_button(app, ui, st, len, false);
        // The volume keeps its place at the right end; the note about the cover gets what's left.
        let volume_w = 12.0 * S1 + super::widgets::text_width(ui, "volume", font::UI) + 2.0 * S2;
        ui.allocate_ui_with_layout(vec2((ui.available_width() - volume_w).max(0.0), SLOT), Layout::left_to_right(Align::Center), |ui| cover_suggestion(app, ui));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| volume_control(app, ui, true));
    });
}

/// Previous, play/pause and next: `small` across, play `large`.
fn buttons(app: &mut App, ui: &mut Ui, st: &Status, small: f32, large: f32) {
    let slot = large.max(SLOT.min(MINI));
    if media_button(ui, Media::Previous, small, slot, false).on_hover_text("Previous (P)").clicked() {
        app.prev();
    }
    let playing = !st.paused && st.path.is_some();
    let toggle = media_button(ui, if playing { Media::Pause } else { Media::Play }, large, slot, true).on_hover_text("Play / pause (Space)");
    if toggle.clicked() {
        // After the queue ended there is nothing loaded: start the current track again.
        if st.path.is_none() { app.play(app.queue.clone(), app.qpos) } else { app.audio.toggle() }
    }
    if media_button(ui, Media::Next, small, slot, false).on_hover_text("Next (N)").clicked() {
        app.next();
    }
}

/// The volume slider, after a speaker or (in the full player) the word.
fn volume_control(app: &mut App, ui: &mut Ui, word: bool) {
    let mut v = app.saved.volume;
    if volume(ui, &mut v) {
        app.saved.volume = v;
        app.audio.volume(v);
    }
    if word {
        muted(ui, "volume");
    }
}

/// Where the queue stands ("2 of 6"), beside the cassette winding on (full player) or alone
/// (compact); a click opens the queue: what played, what plays, what's next.
fn queue_button(app: &mut App, ui: &mut Ui, st: &Status, len: Duration, compact: bool) {
    let ink = ink(ui);
    let inner = ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = S1;
        if !compact {
            let playing = !st.paused && st.path.is_some();
            let played = if len.is_zero() { 0.0 } else { st.pos.as_secs_f32() / len.as_secs_f32() };
            super::cassette::tape(ui, S4, playing, played);
        }
        ui.label(RichText::new(format!("{} of {}", app.qpos + 1, app.queue.len())).color(ink.muted));
    });
    let rect = inner.response.rect.expand(S1 / 2.0);
    let resp = ui.interact(rect, egui::Id::unique("queue button"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Up next: what plays after this song");
    super::widgets::name(ui, &resp, egui::Role::Button, "Up next");
    super::widgets::focus_ring(ui, &resp, rect, 4.0);
    if resp.hovered() || resp.has_focus() {
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, ink.line), egui::StrokeKind::Inside);
    }
    let popup = egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::TOP_START)
        .gap(S1)
        .width(440.0)
        .frame(super::theme::dialog_frame(ui).inner_margin(egui::Margin::same(S2 as i8)));
    // The screenshot demo shows it open (PAUSETAPE_SHOT_QUEUE=1).
    #[cfg(debug_assertions)]
    let popup = if std::env::var_os("PAUSETAPE_SHOT").is_some() && std::env::var_os("PAUSETAPE_SHOT_QUEUE").is_some() { popup.open(true) } else { popup };
    popup.show(|ui| up_next(app, ui));
}

/// The queue: what played (faint), what plays (in the accent), what's next, each with a way to
/// move it or take it out; a double click plays one.
fn up_next(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    row(ui, |ui| {
        super::widgets::caps(ui, "up next");
        let left = app.queue.len().saturating_sub(app.qpos + 1);
        muted(ui, if left == 0 { "nothing after this song".to_string() } else { super::text::plural(left, "song") });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if left > 0 && super::widgets::icon_link(ui, super::icons::Icon::Close, "Clear what's next").clicked() {
                app.clear_upcoming();
            }
        });
    });
    ui.add_space(S1);
    enum Do {
        Jump(usize),
        Remove(usize),
        Move(usize, usize),
    }
    let mut todo = None;
    let n = app.queue.len();
    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
        for (pos, &t) in app.queue.iter().enumerate() {
            let track = &app.tracks[t];
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            super::widgets::row_background(ui, &resp, rect, false);
            let (now, past) = (pos == app.qpos, pos < app.qpos);
            let color = if now { ink.accent } else if past { ink.muted } else { ink.text };
            let p = ui.painter_at(rect);
            let y = rect.center().y;
            p.text(pos2(rect.left() + S3, y), Align2::RIGHT_CENTER, if now { "▸".to_string() } else { (pos + 1).to_string() }, FontId::proportional(font::UI), if now { ink.accent } else { ink.muted });
            let mut words = track.display_title();
            if let Some(a) = &track.artist {
                words = format!("{words}  ·  {a}");
            }
            let controls_w = if now { 0.0 } else { 3.0 * ROW };
            let g = super::widgets::truncated(ui, words, FontId::proportional(font::UI), color, rect.width() - S4 - S5 - controls_w);
            p.galley(pos2(rect.left() + S4, y - g.size().y / 2.0), g, color);
            p.text(pos2(rect.right() - controls_w - S1, y), Align2::RIGHT_CENTER, text::duration(track.duration), super::widgets::data(), ink.muted);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, track.display_title()));
            if resp.double_clicked() {
                todo = Some(Do::Jump(pos));
            }
            if !now {
                // Three slots at the right end: up, down, take out (up and down only where they lead).
                let mut small = |slot: f32, icon, label: &str| {
                    let at = egui::Rect::from_min_size(pos2(rect.right() - (3.0 - slot) * ROW, rect.top()), vec2(ROW, ROW));
                    ui.put(at, |ui: &mut Ui| super::widgets::icon_button(ui, icon, label)).clicked()
                };
                if pos > 0 && small(0.0, super::icons::Icon::Up, "Move up") {
                    todo = Some(Do::Move(pos, pos - 1));
                }
                if pos + 1 < n && small(1.0, super::icons::Icon::Down, "Move down") {
                    todo = Some(Do::Move(pos, pos + 1));
                }
                if small(2.0, super::icons::Icon::Close, "Take out of the queue") {
                    todo = Some(Do::Remove(pos));
                }
            }
        }
    });
    match todo {
        Some(Do::Jump(pos)) => app.audio.jump(pos),
        Some(Do::Remove(pos)) => app.remove_from_queue(pos),
        Some(Do::Move(from, to)) => app.move_in_queue(from, to),
        None => {}
    }
}

/// When the cover shown comes from an image file rather than the songs' tags, offer to add it:
/// players (the Echo Mini's stock firmware among them) show embedded covers, not image files in the folder.
/// The words shorten with "…" to the room they're given (whole on hover); the link always shows.
fn cover_suggestion(app: &mut App, ui: &mut Ui) {
    let Some(ti) = app.current() else { return };
    let t = &app.tracks[ti];
    // Looked for and nothing to show (none in the songs or beside them, or one that can't be
    // read): offer to find one, here where it's missed.
    let shown = app.cover.as_ref().filter(|(p, ..)| *p == t.path);
    if shown.is_some_and(|(_, tex, _)| tex.is_none()) {
        if app.busy.is_some() {
            return;
        }
        let Some(&album) = app.albums_of(&[ti].into()).first() else { return };
        ui.add_space(S2);
        let link_w = super::widgets::text_width(ui, "Find cover", font::UI) + S2;
        if ui.available_width() >= link_w + 120.0 {
            let words = "no cover to show ·";
            super::widgets::fitted(ui, RichText::new(words).color(ink(ui).muted), ui.available_width() - link_w, Sense::hover());
        }
        let hint = "Looks in the album's folder, then on the Cover Art Archive (MusicBrainz). What's found is added to the songs at the next Sync, and to the files with \"write corrected tags\".";
        if ui.available_width() >= link_w && super::widgets::link(ui, "Find cover").on_hover_text(hint).clicked() {
            app.find_covers(vec![album]);
        }
        return;
    }
    let Some(file) = shown.filter(|(_, tex, _)| tex.is_some()).and_then(|(_, _, f)| f.clone()) else { return };
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    ui.add_space(S2);
    // The words, shortened to the room there is, then the link; with too little room for words,
    // a link that says it all; with too little for that, nothing.
    let say = |ui: &mut Ui, words: String, link: &str, alone: &str, hint: &str| -> bool {
        let link_w = super::widgets::text_width(ui, link, font::UI) + S2;
        if ui.available_width() >= link_w + 120.0 {
            let room = ui.available_width() - link_w;
            super::widgets::fitted(ui, RichText::new(&words).color(ink(ui).muted), room, Sense::hover()).on_hover_text(&words);
            super::widgets::link(ui, link).on_hover_text(hint).clicked()
        } else if ui.available_width() >= super::widgets::text_width(ui, alone, font::UI) {
            super::widgets::link(ui, alone).on_hover_text(format!("{words}\n{hint}")).clicked()
        } else {
            false
        }
    };
    if t.cover.is_some() {
        let when = if t.cue.is_some() { "in card copies when you next write" } else { "in card copies when you next write, and in the music files with \"write corrected tags\"" };
        if say(ui, format!("cover from {name} will be added to the songs' tags ({when}) ·"), "Don't", "Don't add", "keep the songs as they are") {
            let album = app.index.artists.iter().flat_map(|a| &a.albums).find(|al| al.tracks.contains(&ti));
            let songs = album.map(|al| al.tracks.clone()).unwrap_or_else(|| vec![ti]);
            app.drop_cover(&songs);
        }
        return;
    }
    let hint = "Embed this image as the cover of the album's songs (resized to 600 px JPEG). Most players, the Echo Mini's stock firmware among them, show embedded covers, not image files in the folder.";
    if say(ui, format!("cover from {name}, not in the songs' tags ·"), "Add it", "Add cover", hint) {
        app.add_cover(ti, file);
    }
}

/// Space, ←/→ seek 10 s, N/P for next/previous, unless typing in a field. While a control
/// has the keyboard's focus (Tab), Space presses it and the arrows move between controls.
fn keyboard(app: &mut App, ui: &Ui, st: &Status, len: Duration) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let focused = ui.ctx().memory(|m| m.focused().is_some());
    let (space, left, right, n, prev) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Space),
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.key_pressed(egui::Key::N),
            i.key_pressed(egui::Key::P),
        )
    });
    if space && !focused {
        app.audio.toggle();
    }
    if left && !focused {
        app.audio.seek(st.pos.saturating_sub(Duration::from_secs(10)));
    }
    if right && !focused {
        app.audio.seek((st.pos + Duration::from_secs(10)).min(len));
    }
    if n {
        app.next();
    }
    if prev {
        app.prev();
    }
}

/// A hairline volume control.
fn volume(ui: &mut Ui, v: &mut f32) -> bool {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(12.0 * S1, S2), Sense::click_and_drag());
    let y = rect.center().y;
    ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, ink.faint));
    ui.painter().hline(rect.left()..=rect.left() + rect.width() * *v, y, Stroke::new(1.5, ink.muted));
    ui.painter().circle_filled(pos2(rect.left() + rect.width() * *v, y), 3.0, ink.text);
    if let Some(pos) = resp.interact_pointer_pos() {
        *v = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        return true;
    }
    false
}
