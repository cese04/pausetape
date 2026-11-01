//! Small reusable controls drawn in the app's style: quiet text links (with or without an
//! icon), framed and primary buttons, tabs, section headings, help text, round transport
//! buttons, labels that shorten to fit.

use egui::{Color32, FontId, Response, RichText, Sense, Stroke, Ui, vec2};

use super::icons::{self, Icon};
use super::theme::{Ink, ink};

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

/// A command: an icon and a word, muted until hovered, on a soft background when hovered.
/// Commands read as commands this way; plain `link`s stay for small in-text actions.
pub fn icon_link(ui: &mut Ui, icon: Icon, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.into(), FontId::proportional(13.5), Color32::PLACEHOLDER);
    let (icon_w, gap, pad) = (13.0, 5.0, vec2(5.0, 2.0));
    let size = vec2(icon_w + gap + galley.size().x, galley.size().y.max(icon_w)) + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let color = if resp.hovered() { ink.text } else { ink.muted };
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ink.hover);
    }
    let inner = rect.shrink2(pad);
    icons::paint(ui.painter(), icons::slot(inner, icon_w), icon, color);
    ui.painter().galley(egui::pos2(inner.left() + icon_w + gap, inner.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Just an icon, for compact commands (move up, remove from list); give it a hover text.
pub fn icon_button(ui: &mut Ui, icon: Icon) -> Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ink.hover);
    }
    icons::paint(ui.painter(), rect.shrink(5.0), icon, if resp.hovered() { ink.text } else { ink.muted });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A tab in the top bar: icon and name in a pill, filled when it's the open one.
pub fn nav_tab(ui: &mut Ui, icon: Icon, text: &str, active: bool) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(15.0), Color32::PLACEHOLDER);
    let (icon_w, gap, pad) = (15.0, 7.0, vec2(12.0, 6.0));
    let (rect, resp) = ui.allocate_exact_size(vec2(icon_w + gap + galley.size().x, galley.size().y.max(icon_w)) + pad * 2.0, Sense::click());
    // The open tab's pill takes a hint of the accent, and its icon the accent itself.
    let fill = if active { ink.chip.lerp_to_gamma(ink.accent, 0.12) } else if resp.hovered() { ink.hover } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
    let color = if active || resp.hovered() { ink.text } else { ink.muted };
    let inner = rect.shrink2(pad);
    icons::paint(ui.painter(), icons::slot(inner, icon_w), icon, if active { ink.accent } else { color });
    ui.painter().galley(egui::pos2(inner.left() + icon_w + gap, inner.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A fact that leads somewhere: icon and text in an outlined pill.
pub fn chip(ui: &mut Ui, icon: Icon, text: impl Into<String>, color: Color32) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.into(), FontId::proportional(13.0), Color32::PLACEHOLDER);
    let (icon_w, gap, pad) = (12.0, 6.0, vec2(9.0, 3.0));
    let (rect, resp) = ui.allocate_exact_size(vec2(icon_w + gap + galley.size().x, galley.size().y.max(icon_w)) + pad * 2.0, Sense::click());
    let fill = if resp.hovered() { ink.hover } else { Color32::TRANSPARENT };
    ui.painter().rect(rect, rect.height() / 2.0, fill, Stroke::new(1.0, if resp.hovered() { color } else { ink.faint }), egui::StrokeKind::Inside);
    let inner = rect.shrink2(pad);
    icons::paint(ui.painter(), icons::slot(inner, icon_w), icon, color);
    ui.painter().galley(egui::pos2(inner.left() + icon_w + gap, inner.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A section heading: icon, small capitals, and a hairline to the edge.
pub fn section(ui: &mut Ui, icon: Icon, title: &str) {
    let ink = ink(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        icons::paint(ui.painter(), rect, icon, ink.muted);
        let r = ui.label(RichText::new(title.to_uppercase()).size(12.0).extra_letter_spacing(1.3).color(ink.text));
        ui.painter().hline(r.rect.right() + 10.0..=ui.max_rect().right(), r.rect.center().y, Stroke::new(0.5, ink.faint));
    });
}

/// One setting: its name in a column on the left, the controls (and their help) on the right.
pub fn setting(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    const LABEL_W: f32 = 170.0;
    let ink = ink(ui);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(LABEL_W, 20.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(LABEL_W);
            ui.add(egui::Label::new(RichText::new(label).color(ink.text)).wrap());
        });
        ui.vertical(add);
    });
    ui.add_space(12.0);
}

/// Explanatory text under a setting: smaller than what it explains, so the setting reads first.
pub fn help(ui: &mut Ui, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    ui.add(egui::Label::new(RichText::new(text.into()).size(13.0).color(ink.muted)).wrap())
}

/// Navigation tab: the active one is set in full ink, others muted.
pub fn tab(ui: &mut Ui, text: &str, active: bool) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(15.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(0.0, 3.0), Sense::click());
    let color = if active || resp.hovered() { ink.text } else { ink.muted };
    ui.painter().galley(rect.min, galley, color);
    if active {
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.5, ink.accent));
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
    framed(ui, None, text.into(), false)
}

/// The one button per screen that does the main thing: solid ink, paper-coloured text.
pub fn primary_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    framed(ui, None, text.into(), true)
}

/// `button` with an icon before the words.
pub fn button_with(ui: &mut Ui, icon: Icon, text: impl Into<String>) -> Response {
    framed(ui, Some(icon), text.into(), false)
}

/// `primary_button` with an icon before the words.
pub fn primary_with(ui: &mut Ui, icon: Icon, text: impl Into<String>) -> Response {
    framed(ui, Some(icon), text.into(), true)
}

fn framed(ui: &mut Ui, icon: Option<Icon>, text: String, primary: bool) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text, FontId::proportional(14.5), Color32::PLACEHOLDER);
    let (icon_w, gap) = if icon.is_some() { (14.0, 7.0) } else { (0.0, 0.0) };
    let pad = if primary { vec2(14.0, 7.0) } else { vec2(12.0, 6.0) };
    let (rect, resp) = ui.allocate_exact_size(vec2(icon_w + gap + galley.size().x, galley.size().y) + pad * 2.0, Sense::click());
    let fg = if primary {
        ui.painter().rect_filled(rect, 5.0, if resp.hovered() { pressed(&ink) } else { ink.accent });
        ink.bg
    } else {
        let stroke = if resp.hovered() { ink.text } else { ink.muted };
        ui.painter().rect(rect, 5.0, if resp.hovered() { ink.hover } else { Color32::TRANSPARENT }, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
        ink.text
    };
    let inner = rect.shrink2(pad);
    if let Some(icon) = icon {
        icons::paint(ui.painter(), icons::slot(inner, icon_w), icon, fg);
    }
    ui.painter().galley(egui::pos2(inner.left() + icon_w + gap, inner.center().y - galley.size().y / 2.0), galley, fg);
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
        (true, false) => (ink.accent, ink.bg),
        (true, true) => (pressed(&ink), ink.bg),
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
    ui.painter().hline(rect.left()..=rect.left() + width * frac.clamp(0.0, 1.0), y, Stroke::new(1.5, ink.accent));
}

/// A colour to choose: a filled dot, ringed when it's the chosen one.
pub fn swatch(ui: &mut Ui, colour: Color32, chosen: bool) -> Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let c = rect.center();
    ui.painter().circle_filled(c, 8.0, colour);
    if chosen || resp.hovered() {
        ui.painter().circle_stroke(c, 11.0, Stroke::new(1.5, if chosen { ink.text } else { ink.faint }));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A filled button under the pointer: the accent a step towards the ink, so its words stay as
/// readable as before (graphite, already the ink, steps towards grey instead).
fn pressed(ink: &Ink) -> Color32 {
    if ink.accent == ink.text { ink.muted } else { ink.accent.lerp_to_gamma(ink.text, 0.25) }
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

