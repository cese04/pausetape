//! Small reusable controls drawn in the app's style: quiet text links, framed and primary
//! buttons, tabs, round transport buttons, labels that shorten to fit.

use egui::{Color32, FontId, Response, RichText, Sense, Stroke, Ui, vec2};

use super::theme::ink;

/// A quiet text link: muted, darkening and underlined on hover.
pub fn link(ui: &mut Ui, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.into(), FontId::proportional(13.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
    let color = if resp.hovered() { ink.text } else { ink.muted };
    if resp.hovered() {
        ui.painter().hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(0.8, color));
    }
    ui.painter().galley(rect.min, galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Navigation tab: the active one is set in full ink, others muted.
pub fn tab(ui: &mut Ui, text: &str, active: bool) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(15.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(0.0, 3.0), Sense::click());
    let color = if active || resp.hovered() { ink.text } else { ink.muted };
    ui.painter().galley(rect.min, galley, color);
    if active {
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, ink.text));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Small-caps style section label.
pub fn caps(ui: &mut Ui, text: &str) {
    let ink = ink(ui);
    ui.label(egui::RichText::new(text.to_uppercase()).size(11.0).extra_letter_spacing(1.2).color(ink.muted));
}

pub fn muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    ui.label(egui::RichText::new(text.into()).color(ink.muted))
}

/// A framed button for primary actions; quieter `link`s remain for secondary ones.
pub fn button(ui: &mut Ui, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.into(), FontId::proportional(14.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(24.0, 12.0), Sense::click());
    let stroke = if resp.hovered() { ink.text } else { ink.muted };
    ui.painter().rect(rect, 5.0, if resp.hovered() { ink.hover } else { Color32::TRANSPARENT }, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink.text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The one button per screen that does the main thing: solid ink, paper-coloured text.
pub fn primary_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.into(), FontId::proportional(14.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(28.0, 14.0), Sense::click());
    let fill = if resp.hovered() { ink.muted } else { ink.text };
    ui.painter().rect_filled(rect, 5.0, fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink.bg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[derive(Clone, Copy)]
pub enum Media {
    Previous,
    Play,
    Pause,
    Next,
}

/// Round transport button with a drawn (font-independent) icon. The circle is `size` across and
/// centred in a `slot` tall space, so buttons of different sizes in one row share a centre line.
pub fn media_button(ui: &mut Ui, icon: Media, size: f32, slot: f32, primary: bool) -> Response {
    use egui::{Pos2, Shape, pos2};
    let ink = ink(ui);
    let (slot_rect, resp) = ui.allocate_exact_size(vec2(size, slot.max(size)), Sense::click());
    let rect = egui::Rect::from_center_size(slot_rect.center(), vec2(size, size));
    let c = rect.center();
    let r = size / 2.0 - 1.0;
    let (bg, fg) = match (primary, resp.hovered()) {
        (true, false) => (ink.text, ink.bg),
        (true, true) => (ink.muted, ink.bg),
        (false, false) => (Color32::TRANSPARENT, ink.text),
        (false, true) => (ink.hover, ink.text),
    };
    ui.painter().circle(c, r, bg, Stroke::new(1.0, if primary { bg } else { ink.faint }));
    let s = size * 0.2;
    let tri = |pts: Vec<Pos2>| Shape::convex_polygon(pts, fg, Stroke::NONE);
    let bar = |x: f32, w: f32| egui::Rect::from_min_max(pos2(x, c.y - s), pos2(x + w, c.y + s));
    let p = ui.painter();
    match icon {
        Media::Play => {
            p.add(tri(vec![pos2(c.x - s * 0.7, c.y - s), pos2(c.x + s, c.y), pos2(c.x - s * 0.7, c.y + s)]));
        }
        Media::Pause => {
            p.rect_filled(bar(c.x - s * 0.8, s * 0.55), 1.0, fg);
            p.rect_filled(bar(c.x + s * 0.25, s * 0.55), 1.0, fg);
        }
        Media::Previous => {
            p.rect_filled(bar(c.x - s, s * 0.3), 0.0, fg);
            p.add(tri(vec![pos2(c.x + s * 0.9, c.y - s), pos2(c.x + s * 0.9, c.y + s), pos2(c.x - s * 0.6, c.y)]));
        }
        Media::Next => {
            p.rect_filled(bar(c.x + s * 0.7, s * 0.3), 0.0, fg);
            p.add(tri(vec![pos2(c.x - s * 0.9, c.y - s), pos2(c.x + s * 0.6, c.y), pos2(c.x - s * 0.9, c.y + s)]));
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small drawn folder (fonts rarely have a folder glyph). Allocates `size` square.
pub fn folder_icon(ui: &mut Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_folder(ui.painter(), rect, color);
}

pub fn paint_folder(p: &egui::Painter, rect: egui::Rect, color: Color32) {
    let w = rect.width();
    let body = egui::Rect::from_min_max(rect.min + vec2(0.0, w * 0.28), rect.max - vec2(0.0, w * 0.1));
    let tab = egui::Rect::from_min_size(rect.min + vec2(0.0, w * 0.14), vec2(w * 0.45, w * 0.2));
    p.rect_stroke(tab, 1.5, Stroke::new(1.2, color), egui::StrokeKind::Inside);
    p.rect_stroke(body, 1.5, Stroke::new(1.2, color), egui::StrokeKind::Inside);
}

/// Width `text` would take in the proportional font at `size`.
pub fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(size), Color32::PLACEHOLDER).size().x
}

/// A label that never takes more than `max_w`, ending in "…" when it has to.
pub fn fitted(ui: &mut Ui, text: egui::RichText, max_w: f32, sense: Sense) -> Response {
    ui.scope(|ui| {
        ui.set_max_width(max_w.max(40.0));
        ui.add(egui::Label::new(text).truncate().sense(sense))
    })
    .inner
}

pub fn hairline_progress(ui: &mut Ui, frac: f32, width: f32) {
    let ink = ink(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 10.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, ink.faint));
    ui.painter().hline(rect.left()..=rect.left() + width * frac.clamp(0.0, 1.0), y, Stroke::new(1.5, ink.text));
}

pub fn truncated(ui: &Ui, text: String, font: FontId, color: Color32, max_w: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text, font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(40.0));
    ui.painter().layout_job(job)
}

pub trait StrikeIf {
    fn strikethrough_if(self, on: bool) -> Self;
}

impl StrikeIf for RichText {
    fn strikethrough_if(self, on: bool) -> Self {
        if on { self.strikethrough() } else { self }
    }
}

