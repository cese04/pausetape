//! The cassette the app is named after, in miniature: beside its name, as the reels that turn
//! while work is under way, and in the player, where the tape passes from one reel to the other
//! as the song plays.

use std::f32::consts::TAU;

use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use super::theme::{Ink, ink};

/// The colour of tape wound on a reel: the muted ink, thinned, so the hubs stand out on it.
fn tape_colour(ink: &Ink) -> Color32 {
    ink.muted.gamma_multiply(0.45)
}

/// How reels are drawn: the tape's colour, and the hub's colour and line width.
struct Look {
    tape: Color32,
    hub: Color32,
    width: f32,
}

/// One reel: the tape wound on it (`pack`, 0 bare to 1 full, by length, so by area), and the hub,
/// its three teeth at `turn` radians.
fn reel(p: &Painter, c: Pos2, outer: f32, pack: f32, turn: f32, look: &Look) {
    let core = outer * 0.45;
    let r = (core * core + pack.clamp(0.0, 1.0) * (outer * outer - core * core)).sqrt();
    let hub = Stroke::new(look.width, look.hub);
    p.circle_filled(c, r, look.tape);
    p.circle_stroke(c, core, hub);
    for k in 0..3 {
        let a = turn + k as f32 * TAU / 3.0;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([c + d * core * 0.35, c + d * core], hub);
    }
}

/// Work under way: two reels turning, in place of a spinner. `height` is a reel's diameter.
pub fn spinner(ui: &mut Ui, height: f32) -> Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(height * 2.0 + height * 0.3, height), Sense::hover());
    // Still when motion is reduced: the reels say "under way" as they are.
    let still = super::motion::reduced(ui.ctx());
    let turn = if still { 0.0 } else { ui.input(|i| i.time) as f32 * 4.0 };
    let r = height / 2.0;
    let look = Look { tape: tape_colour(&ink), hub: ink.accent, width: 1.3 };
    for (cx, pack) in [(rect.left() + r, 0.7), (rect.right() - r, 0.3)] {
        reel(ui.painter(), pos2(cx, rect.center().y), r, pack, turn, &look);
    }
    if !still {
        ui.ctx().request_repaint();
    }
    resp
}

/// A cassette's outline in `rect`: shell, label line, window, the reels in it, and the tape
/// along the window's foot. `played` (0..=1) has wound that much onto the right reel.
fn shell(p: &Painter, rect: Rect, turns: (f32, f32), played: f32, ink: &Ink, hub: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| rect.min + vec2(x * w, y * h);
    let line = Stroke::new(1.0, ink.muted);
    p.rect_stroke(rect, h * 0.12, line, egui::StrokeKind::Inside);
    // The label's ruled line, and the step down at the bottom where the heads meet the tape.
    p.line_segment([at(0.12, 0.2), at(0.88, 0.2)], Stroke::new(1.0, ink.faint));
    p.add(egui::Shape::line(vec![at(0.24, 1.0), at(0.29, 0.87), at(0.71, 0.87), at(0.76, 1.0)], line));
    let window = Rect::from_min_max(at(0.15, 0.3), at(0.85, 0.8));
    p.rect_stroke(window, h * 0.08, Stroke::new(1.0, ink.faint), egui::StrokeKind::Inside);
    let outer = window.height() * 0.46;
    let (left, right) = (at(0.32, 0.55), at(0.68, 0.55));
    let look = Look { tape: tape_colour(ink), hub, width: 1.1 };
    reel(p, left, outer, 1.0 - played, turns.0, &look);
    reel(p, right, outer, played, turns.1, &look);
}

/// The cassette beside the app's name: still.
pub fn mark(ui: &mut Ui, height: f32) -> Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(height * 1.6, height), Sense::hover());
    shell(ui.painter(), rect.shrink(0.5), (0.3, 1.2), 0.35, &ink, ink.text);
    resp
}

/// The player's cassette: the reels turn while the song plays, each faster the less tape it
/// holds (the tape runs at one speed past the heads), and the tape passes from the left reel
/// to the right as the song goes on.
pub fn tape(ui: &mut Ui, height: f32, playing: bool, played: f32) -> Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(height * 1.6, height), Sense::hover());
    let id = resp.id.with("turns");
    let mut turns: (f32, f32) = ui.ctx().data(|d| d.get_temp(id)).unwrap_or((0.0, 0.7));
    if playing && !super::motion::reduced(ui.ctx()) {
        // An angle kept from frame to frame: speed changes with the pack, so time alone won't do.
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let speed = |pack: f32| 2.4 / (0.45 + 0.55 * pack.clamp(0.0, 1.0).sqrt());
        turns.0 = (turns.0 + dt * speed(1.0 - played)) % TAU;
        turns.1 = (turns.1 + dt * speed(played)) % TAU;
        ui.ctx().data_mut(|d| d.insert_temp(id, turns));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
    }
    shell(ui.painter(), rect.shrink(0.5), turns, played.clamp(0.0, 1.0), &ink, if playing { ink.accent } else { ink.muted });
    resp
}
