//! Small reusable controls drawn in the app's style: quiet text links (with or without an
//! icon), framed and primary buttons, tabs, section headings, help text, round transport
//! buttons, labels that shorten to fit.

use egui::{Color32, FontId, Rect, Response, RichText, Sense, Stroke, Ui, Role, WidgetInfo, pos2, vec2};

use super::icons::{self, Icon};
use super::layout::{CONTROL, ICON, ROW, S1, S2, S3, row};
use super::theme::{Ink, ink};

/// What a control drawn here is called, for screen readers.
pub fn name(ui: &Ui, resp: &Response, role: Role, text: &str) {
    resp.widget_info(|| WidgetInfo::labeled(role, ui.is_enabled(), text));
}

/// Keyboard focus (Tab gives it; a click doesn't): a ring in the accent just outside the
/// control, so it shows where Space or Enter would act.
pub fn focus_ring(ui: &Ui, resp: &Response, rect: Rect, radius: f32) {
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), radius + 2.0, Stroke::new(2.0, ink(ui).accent), egui::StrokeKind::Outside);
    }
}

/// The background of a song or album row: the selection's tint with a bar of the accent at
/// its left edge, or a darker step under the pointer or the keyboard's focus.
pub fn row_background(ui: &Ui, resp: &Response, rect: Rect, selected: bool) {
    let ink = ink(ui);
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 4.0, ink.selected);
        p.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 1.0, ink.accent);
    } else if hot(resp) {
        p.rect_filled(rect, 4.0, ink.row_hover);
    }
}

/// Under the pointer or the keyboard's focus: either way, the control shows it's the one.
fn hot(resp: &Response) -> bool {
    resp.hovered() || resp.has_focus()
}

/// A quiet text link: muted, darkening and underlined on hover. A row tall, the words centred.
pub fn link(ui: &mut Ui, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    let text = text.into();
    let galley = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(13.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x, ROW), Sense::click());
    name(ui, &resp, Role::Link, &text);
    focus_ring(ui, &resp, rect, 2.0);
    let color = if hot(&resp) { ink.text } else { ink.muted };
    let top = rect.center().y - galley.size().y / 2.0;
    if hot(&resp) {
        ui.painter().hline(rect.x_range(), top + galley.size().y - 1.0, Stroke::new(0.8, color));
    }
    ui.painter().galley(pos2(rect.left(), top), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// An icon in its `ICON` square, `inset` in from its edges, centred on `y` from `x`.
fn icon_at(ui: &Ui, x: f32, y: f32, inset: f32, icon: Icon, color: Color32) {
    let square = Rect::from_min_size(pos2(x, y - ICON / 2.0), vec2(ICON, ICON));
    icons::paint(ui.painter(), square.shrink(inset), icon, color);
}

/// Words laid out at `x`, centred on `y`.
fn words_at(ui: &Ui, x: f32, y: f32, galley: std::sync::Arc<egui::Galley>, color: Color32) {
    let top = y - galley.size().y / 2.0;
    ui.painter().galley(pos2(x, top), galley, color);
}

/// A command: an icon and a word, muted until hovered, on a soft background when hovered.
/// Commands read as commands this way; plain `link`s stay for small in-text actions.
pub fn icon_link(ui: &mut Ui, icon: Icon, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    let text = text.into();
    let galley = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(13.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(vec2(S1 + ICON + S1 + galley.size().x + S1, ROW), Sense::click());
    name(ui, &resp, Role::Button, &text);
    focus_ring(ui, &resp, rect, 4.0);
    let color = if hot(&resp) { ink.text } else { ink.muted };
    if hot(&resp) {
        ui.painter().rect_filled(rect, 4.0, ink.hover);
    }
    let y = rect.center().y;
    icon_at(ui, rect.left() + S1, y, 1.5, icon, color);
    words_at(ui, rect.left() + S1 + ICON + S1, y, galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// An icon on its own, not a control, in a row beside words.
pub fn icon(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ICON, ROW), Sense::hover());
    icon_at(ui, rect.left(), rect.center().y, 1.5, icon, color);
}

/// Just an icon, for compact commands (move up, remove from list). `label` says what it does,
/// on hover and to screen readers.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ROW, ROW), Sense::click());
    name(ui, &resp, Role::Button, label);
    focus_ring(ui, &resp, rect, 4.0);
    if hot(&resp) {
        ui.painter().rect_filled(rect, 4.0, ink.hover);
    }
    icons::paint(ui.painter(), rect.shrink((ROW - ICON) / 2.0 + 1.0), icon, if hot(&resp) { ink.text } else { ink.muted });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(label)
}

/// A tab in the top bar: icon and name in a pill, filled when it's the open one. Without
/// `show_name` (a narrow window) only the icon is drawn; the name is still its tooltip and
/// what screen readers say.
pub fn nav_tab(ui: &mut Ui, icon: Icon, text: &str, show_name: bool, active: bool) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(if show_name { text.to_string() } else { String::new() }, FontId::proportional(15.0), Color32::PLACEHOLDER);
    let words = if show_name { S1 + galley.size().x } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(S2 + ICON + words + S2, CONTROL), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::Tab, ui.is_enabled(), active, text));
    focus_ring(ui, &resp, rect, rect.height() / 2.0);
    // The open tab's pill takes a hint of the accent, and its icon the accent itself.
    let fill = if active { ink.chip.lerp_to_gamma(ink.accent, 0.12) } else if hot(&resp) { ink.hover } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
    let color = if active || hot(&resp) { ink.text } else { ink.muted };
    let y = rect.center().y;
    icon_at(ui, rect.left() + S2, y, 0.5, icon, if active { ink.accent } else { color });
    words_at(ui, rect.left() + S2 + ICON + S1, y, galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A fact that leads somewhere: icon and text in an outlined pill.
pub fn chip(ui: &mut Ui, icon: Icon, text: impl Into<String>, color: Color32) -> Response {
    let ink = ink(ui);
    let text = text.into();
    let galley = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(13.0), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(vec2(S1 + ICON + S1 + galley.size().x + S1 + S1, ROW), Sense::click());
    name(ui, &resp, Role::Button, &text);
    focus_ring(ui, &resp, rect, rect.height() / 2.0);
    let fill = if hot(&resp) { ink.hover } else { Color32::TRANSPARENT };
    ui.painter().rect(rect, rect.height() / 2.0, fill, Stroke::new(1.0, if hot(&resp) { color } else { ink.faint }), egui::StrokeKind::Inside);
    let y = rect.center().y;
    icon_at(ui, rect.left() + S1, y, 2.0, icon, color);
    words_at(ui, rect.left() + S1 + ICON + S1, y, galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A search box. Once something is typed, a clear button sits inside its right end (drawn
/// over the box, so nothing beside it moves); Esc clears it too while typing.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let ink = ink(ui);
    let edit = egui::TextEdit::singleline(text)
        .hint_text(hint)
        .desired_width(width)
        .min_size(vec2(0.0, ROW))
        .align(egui::Align2::LEFT_CENTER)
        .margin(egui::Margin { left: S1 as i8, right: S3 as i8, top: 0, bottom: 0 });
    let r = ui.add(edit);
    if (r.has_focus() || r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        text.clear();
    }
    if !text.is_empty() {
        let rect = Rect::from_center_size(pos2(r.rect.right() - S3 / 2.0, r.rect.center().y), vec2(ICON, ICON));
        let clear = ui.interact(rect, r.id.with("clear"), Sense::click()).on_hover_text("clear (Esc)").on_hover_cursor(egui::CursorIcon::PointingHand);
        name(ui, &clear, Role::Button, "clear the search");
        focus_ring(ui, &clear, rect, ICON / 2.0);
        if hot(&clear) {
            ui.painter().circle_filled(rect.center(), ICON / 2.0, ink.faint);
        }
        icons::paint(ui.painter(), rect.shrink(4.0), Icon::Close, if hot(&clear) { ink.text } else { ink.muted });
        if clear.clicked() {
            text.clear();
        }
    }
    r
}

/// A word that describes, not a control: a genre, in a small filled pill.
pub fn tag(ui: &mut Ui, text: impl Into<String>) -> Response {
    let ink = ink(ui);
    let galley = ui.painter().layout_no_wrap(text.into(), FontId::proportional(12.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(vec2(S1 + galley.size().x + S1, ROW), Sense::hover());
    ui.painter().rect_filled(rect, rect.height() / 2.0, ink.chip);
    words_at(ui, rect.left() + S1, rect.center().y, galley, ink.text);
    resp
}

/// A section heading: icon, small capitals, and a hairline to the edge.
pub fn section(ui: &mut Ui, icon: Icon, title: &str) {
    let ink = ink(ui);
    row(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(ICON, ICON), Sense::hover());
        icons::paint(ui.painter(), rect.shrink(1.0), icon, ink.muted);
        let r = ui.label(RichText::new(title.to_uppercase()).size(12.0).extra_letter_spacing(1.3).color(ink.text));
        ui.painter().hline(r.rect.right() + S1..=ui.max_rect().right(), r.rect.center().y, Stroke::new(0.5, ink.faint));
    });
}

/// The width of the column settings have their names in.
pub const LABEL_W: f32 = 21.0 * S1;

/// One setting: its name in a column on the left, the controls (and their help) on the right.
/// Both columns start at the top: the name lines up with the first row of controls.
pub fn setting(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    let ink = ink(ui);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(LABEL_W, ROW), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_width(LABEL_W);
            ui.add(egui::Label::new(RichText::new(label).color(ink.text)).wrap());
        });
        ui.vertical(add);
    });
    ui.add_space(S2);
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
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x, ROW), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::Tab, ui.is_enabled(), active, text));
    focus_ring(ui, &resp, rect, 2.0);
    let color = if active || hot(&resp) { ink.text } else { ink.muted };
    words_at(ui, rect.left(), rect.center().y, galley, color);
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
    let galley = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(14.5), Color32::PLACEHOLDER);
    let lead = if icon.is_some() { ICON + S1 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(S2 + lead + galley.size().x + S2, CONTROL), Sense::click());
    name(ui, &resp, Role::Button, &text);
    focus_ring(ui, &resp, rect, 6.0);
    let fg = if primary {
        ui.painter().rect_filled(rect, 6.0, if hot(&resp) { pressed(&ink) } else { ink.accent });
        ink.bg
    } else {
        let stroke = if hot(&resp) { ink.text } else { ink.muted };
        ui.painter().rect(rect, 6.0, if hot(&resp) { ink.hover } else { Color32::TRANSPARENT }, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
        ink.text
    };
    let y = rect.center().y;
    if let Some(icon) = icon {
        icon_at(ui, rect.left() + S2, y, 1.0, icon, fg);
    }
    words_at(ui, rect.left() + S2 + lead, y, galley, fg);
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
    let label = match icon {
        Media::Previous => "previous",
        Media::Play => "play",
        Media::Pause => "pause",
        Media::Next => "next",
    };
    name(ui, &resp, Role::Button, label);
    focus_ring(ui, &resp, rect, size / 2.0);
    let c = rect.center();
    let r = size / 2.0 - 1.0;
    let (bg, fg) = match (primary, hot(&resp)) {
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
    let (rect, _) = ui.allocate_exact_size(vec2(width, S1), Sense::hover());
    let y = rect.center().y;
    ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, ink.faint));
    ui.painter().hline(rect.left()..=rect.left() + width * frac.clamp(0.0, 1.0), y, Stroke::new(1.5, ink.accent));
}

/// A colour to choose: a filled dot, ringed when it's the chosen one.
pub fn swatch(ui: &mut Ui, colour: Color32, chosen: bool, label: &str) -> Response {
    let ink = ink(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::RadioButton, ui.is_enabled(), chosen, label));
    focus_ring(ui, &resp, rect.shrink(1.0), 11.0);
    let c = rect.center();
    ui.painter().circle_filled(c, 8.0, colour);
    if chosen || hot(&resp) {
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

