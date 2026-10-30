//! A Tufte-inspired look: warm paper, one serif face, data over chrome. The tab bar is the one
//! band of colour, so the app's frame stands apart from what's in it; elsewhere colour is
//! reserved for warnings and the playhead.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme, Ui, vec2};

pub struct Ink {
    pub bg: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub hover: Color32,
    pub accent: Color32,
    /// The tab bar, a step off the page so it reads as the app's frame, not its content.
    pub bar: Color32,
    /// The selected tab, and other small filled shapes on the bar.
    pub chip: Color32,
}

pub fn ink(ui: &Ui) -> Ink {
    palette(ui.visuals().dark_mode)
}

fn palette(dark: bool) -> Ink {
    if dark {
        Ink {
            bg: Color32::from_rgb(0x15, 0x15, 0x15),
            text: Color32::from_rgb(0xdd, 0xdd, 0xdd),
            muted: Color32::from_rgb(0x8a, 0x8a, 0x8a),
            faint: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            hover: Color32::from_rgb(0x1f, 0x1f, 0x1f),
            accent: Color32::from_rgb(0xe0, 0x6c, 0x6c),
            bar: Color32::from_rgb(0x1d, 0x1d, 0x1d),
            chip: Color32::from_rgb(0x30, 0x30, 0x30),
        }
    } else {
        Ink {
            bg: Color32::from_rgb(0xff, 0xff, 0xf8),
            text: Color32::from_rgb(0x11, 0x11, 0x11),
            muted: Color32::from_rgb(0x77, 0x77, 0x70),
            faint: Color32::from_rgb(0xdd, 0xdd, 0xd2),
            hover: Color32::from_rgb(0xf4, 0xf4, 0xea),
            accent: Color32::from_rgb(0xa0, 0x00, 0x00),
            bar: Color32::from_rgb(0xf2, 0xf2, 0xe6),
            chip: Color32::from_rgb(0xe2, 0xe2, 0xd4),
        }
    }
}

/// System serif faces close in spirit to ET Book, then CJK fallbacks (music libraries are rarely
/// all-Latin). Missing files are skipped, so this degrades to egui's defaults anywhere.
const SERIF: &[(&str, u32)] = &[
    ("/System/Library/Fonts/Palatino.ttc", 0),
    ("/System/Library/Fonts/Supplemental/Georgia.ttf", 0),
    ("C:\\Windows\\Fonts\\pala.ttf", 0),
    ("C:\\Windows\\Fonts\\georgia.ttf", 0),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf", 0),
];

const FALLBACK: &[(&str, u32)] = &[
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0),
    ("C:\\Windows\\Fonts\\msyh.ttc", 0),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
];

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut load = |list: &[(&str, u32)], first_only: bool| -> Vec<String> {
        let mut names = Vec::new();
        for (path, index) in list {
            if let Ok(bytes) = std::fs::read(path) {
                let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
                fonts.font_data.insert(name.clone(), Arc::new(FontData::from_blob(Arc::new(bytes), *index)));
                names.push(name);
                if first_only {
                    break;
                }
            }
        }
        names
    };
    let serif = load(SERIF, true);
    let fallback = load(FALLBACK, false);
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    for (i, name) in serif.into_iter().enumerate() {
        prop.insert(i, name);
    }
    prop.extend(fallback.iter().cloned());
    fonts.families.entry(FontFamily::Monospace).or_default().extend(fallback);
    ctx.set_fonts(fonts);

    for theme in [Theme::Light, Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            style.text_styles = [
                (TextStyle::Heading, FontId::proportional(26.0)),
                (TextStyle::Body, FontId::proportional(15.5)),
                (TextStyle::Button, FontId::proportional(15.0)),
                (TextStyle::Small, FontId::proportional(12.0)),
                (TextStyle::Monospace, FontId::monospace(13.0)),
            ]
            .into();
            style.spacing.item_spacing = vec2(10.0, 6.0);
            style.spacing.button_padding = vec2(6.0, 2.0);
            let v = &mut style.visuals;
            let ink = palette(theme == Theme::Dark);
            v.panel_fill = ink.bg;
            v.window_fill = ink.bg;
            v.extreme_bg_color = ink.hover;
            v.hyperlink_color = ink.text;
            v.selection.bg_fill = ink.faint;
            v.selection.stroke = Stroke::new(1.0, ink.text);
            v.window_stroke = Stroke::new(1.0, ink.faint);
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, ink.faint);
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, ink.text);
            for (w, fg) in [
                (&mut v.widgets.inactive, ink.muted),
                (&mut v.widgets.hovered, ink.text),
                (&mut v.widgets.active, ink.text),
                (&mut v.widgets.open, ink.text),
            ] {
                w.bg_fill = Color32::TRANSPARENT;
                w.weak_bg_fill = Color32::TRANSPARENT;
                w.bg_stroke = Stroke::new(1.0, ink.faint);
                w.fg_stroke = Stroke::new(1.0, fg);
                w.corner_radius = CornerRadius::ZERO;
                w.expansion = 0.0;
            }
            v.widgets.hovered.weak_bg_fill = ink.hover;
        });
    }
}

/// A softly outlined panel used to group one step of a task.
pub fn panel_frame(ui: &Ui) -> egui::Frame {
    let ink = ink(ui);
    egui::Frame::new().fill(ink.hover).stroke(Stroke::new(1.0, ink.faint)).corner_radius(8.0).inner_margin(egui::Margin::same(18))
}

/// Outlined in the accent colour: something the user should know before it surprises them.
pub fn warning_frame(ui: &Ui) -> egui::Frame {
    let ink = ink(ui);
    egui::Frame::new().stroke(Stroke::new(1.0, ink.accent)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(14, 10))
}
