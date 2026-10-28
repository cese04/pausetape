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

/// Side of the cover square; matches the height of title + sparkline + buttons.
const COVER: f32 = 116.0;
/// Every transport button sits in a slot this tall, so different sizes share one centre line.
const SLOT: f32 = 44.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let st = app.audio.status();
    if let Some(e) = &st.error {
        ui.label(RichText::new(e).color(ink.accent).size(12.0));
    }
    let Some(ti) = app.current() else {
        muted(ui, "Nothing playing. Click a track in the library.");
        return;
    };
    let len = st.len.filter(|d| !d.is_zero()).unwrap_or(app.tracks[ti].duration);
    if st.path.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(200));
    }

    ui.horizontal_top(|ui| {
        cover_art(app, ui, ti);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            title_row(app, ui, ti, &st, len);
            scrubber(app, ui, ti, &st, len);
            ui.add_space(2.0);
            transport(app, ui, &st);
        });
    });
    keyboard(app, ui, &st, len);
}

fn cover_art(app: &App, ui: &mut Ui, ti: usize) {
    let ink = ink(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(COVER, COVER), Sense::hover());
    let t = &app.tracks[ti];
    match app.cover.as_ref().filter(|(p, _)| *p == t.path).and_then(|(_, tex)| tex.as_ref()) {
        Some(tex) => {
            // Fit inside the square without cropping; most covers are square anyway.
            let size = tex.size_vec2();
            let scale = (COVER / size.x).min(COVER / size.y);
            let r = egui::Rect::from_center_size(rect.center(), size * scale);
            egui::Image::new(tex).corner_radius(3.0).paint_at(ui, r);
        }
        None => {
            // No art: a quiet square with the album's initial, in the manner of a library card.
            ui.painter().rect(rect, 3.0, ink.hover, Stroke::new(1.0, ink.faint), egui::StrokeKind::Inside);
            let initial = t.album.as_deref().or(t.title.as_deref()).and_then(|s| s.chars().next()).unwrap_or('♪');
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, initial, FontId::proportional(40.0), ink.faint);
        }
    }
}

fn title_row(app: &App, ui: &mut Ui, ti: usize, st: &Status, len: Duration) {
    let ink = ink(ui);
    let t = &app.tracks[ti];
    ui.horizontal(|ui| {
        ui.label(RichText::new(t.display_title()).size(19.0).color(ink.text));
        let meta: Vec<String> = [t.artist.clone(), t.album.clone(), t.year.map(|y| y.to_string())].into_iter().flatten().collect();
        muted(ui, meta.join(" · "));
        if st.converting {
            ui.label(RichText::new("converting with ffmpeg…").size(12.5).color(ink.muted));
        } else if let Some(note) = text::format_note(t) {
            ui.label(RichText::new(text::format_badge(t)).size(12.5).color(ink.accent)).on_hover_text(note);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            muted(ui, format!("{} / {}", text::duration(st.pos), text::duration(len)));
        });
    });
}

fn scrubber(app: &mut App, ui: &mut Ui, ti: usize, st: &Status, len: Duration) {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click_and_drag());
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
            p.hline(rect.left()..=playhead_x, mid, Stroke::new(1.5, ink.text));
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

fn transport(app: &mut App, ui: &mut Ui, st: &Status) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if media_button(ui, Media::Previous, 32.0, SLOT, false).on_hover_text("previous (P)").clicked() {
            app.prev();
        }
        let playing = !st.paused && st.path.is_some();
        let toggle = media_button(ui, if playing { Media::Pause } else { Media::Play }, 42.0, SLOT, true).on_hover_text("play / pause (space)");
        if toggle.clicked() {
            // After the queue ended there is nothing loaded: start the current track again.
            if st.path.is_none() { app.play(app.queue.clone(), app.qpos) } else { app.audio.toggle() }
        }
        if media_button(ui, Media::Next, 32.0, SLOT, false).on_hover_text("next (N)").clicked() {
            app.next();
        }
        ui.add_space(12.0);
        muted(ui, format!("{} of {}", app.qpos + 1, app.queue.len()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut v = app.saved.volume;
            if volume(ui, &mut v) {
                app.saved.volume = v;
                app.audio.volume(v);
            }
            muted(ui, "volume");
        });
    });
}

/// Space, ←/→ seek 10 s, N/P for next/previous, unless typing in a field.
fn keyboard(app: &mut App, ui: &Ui, st: &Status, len: Duration) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let (space, left, right, n, prev) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Space),
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.key_pressed(egui::Key::N),
            i.key_pressed(egui::Key::P),
        )
    });
    if space {
        app.audio.toggle();
    }
    if left {
        app.audio.seek(st.pos.saturating_sub(Duration::from_secs(10)));
    }
    if right {
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
    let (rect, resp) = ui.allocate_exact_size(vec2(90.0, 14.0), Sense::click_and_drag());
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
