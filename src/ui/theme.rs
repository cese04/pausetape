//! A Tufte-inspired look: warm paper, one serif face, data over chrome. The tab bar is the one
//! band of colour, so the app's frame stands apart from what's in it; elsewhere colour is
//! reserved for warnings (always brick red) and the accent the user chooses, which marks where
//! they are and what plays: the open tab, the main button, progress, the playhead.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme, Ui, vec2};
use serde::{Deserialize, Serialize};

use super::layout::{ICON, ROW, S1, S2, S3, S5};

pub struct Ink {
    pub bg: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    /// The edge of a control (a field, a checkbox, an outlined button) where only the edge says
    /// it's there: 3:1 or more against the page, as `faint` (for rules and dividers) isn't.
    pub line: Color32,
    pub hover: Color32,
    /// A song or album row under the pointer: a step darker than `hover`, so the row shows.
    pub row_hover: Color32,
    /// A selected row: a tint of the accent, light enough for `muted` text to stay at 4.5:1;
    /// a bar of the accent at its left edge says it more plainly.
    pub selected: Color32,
    /// The chosen accent: what's open, playing or under way, and the main button.
    pub accent: Color32,
    /// Warnings and problems, whatever the accent.
    pub warn: Color32,
    /// The tab bar, a step off the page so it reads as the app's frame, not its content.
    pub bar: Color32,
    /// The selected tab, and other small filled shapes on the bar.
    pub chip: Color32,
}

/// Accent colours to choose from. Each reads at 4.5:1 or more on every surface of its theme, as
/// text on the page and as the fill under a button's words.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    Brick,
    Ochre,
    Moss,
    #[default]
    Teal,
    Blue,
    Plum,
    Graphite,
}

impl Accent {
    pub const ALL: [Accent; 7] = [Accent::Brick, Accent::Ochre, Accent::Moss, Accent::Teal, Accent::Blue, Accent::Plum, Accent::Graphite];

    pub fn name(self) -> &'static str {
        match self {
            Accent::Brick => "brick",
            Accent::Ochre => "ochre",
            Accent::Moss => "moss",
            Accent::Teal => "teal",
            Accent::Blue => "blue",
            Accent::Plum => "plum",
            Accent::Graphite => "graphite",
        }
    }

    /// How much of the accent a selected row takes: graphite, nearly the ink, takes less.
    fn tint(self) -> f32 {
        if self == Accent::Graphite { 0.07 } else { 0.10 }
    }

    pub fn colour(self, dark: bool) -> Color32 {
        let (light, dark_) = match self {
            Accent::Brick => ([0xa5, 0x40, 0x2f], [0xe4, 0x82, 0x70]),
            Accent::Ochre => ([0x8a, 0x5a, 0x12], [0xd9, 0xa5, 0x4a]),
            Accent::Moss => ([0x4f, 0x6b, 0x2c], [0xa3, 0xbd, 0x78]),
            Accent::Teal => ([0x1f, 0x6e, 0x6a], [0x6c, 0xc0, 0xb5]),
            Accent::Blue => ([0x33, 0x5f, 0x9e], [0x8f, 0xb0, 0xe6]),
            Accent::Plum => ([0x7a, 0x45, 0x79], [0xcf, 0x9b, 0xcb]),
            // The ink itself: the look without a colour.
            Accent::Graphite => ([0x33, 0x30, 0x2b], [0xe4, 0xe0, 0xd8]),
        };
        let [r, g, b] = if dark { dark_ } else { light };
        Color32::from_rgb(r, g, b)
    }
}

fn accent_id() -> egui::Id {
    egui::Id::unique("pausetape accent")
}

/// Make `accent` the one `ink` hands out from now on.
pub fn set_accent(ctx: &egui::Context, accent: Accent) {
    ctx.data_mut(|d| d.insert_temp(accent_id(), accent));
}

pub fn ink(ui: &Ui) -> Ink {
    let accent = ui.ctx().data(|d| d.get_temp::<Accent>(accent_id())).unwrap_or_default();
    palette(ui.visuals().dark_mode, accent)
}

/// Both themes are warm greys: the paper is a little dimmed and the ink is charcoal, not black,
/// so text reads about 12:1 rather than glaring at 19:1. `muted` stays at 4.5:1 or more on every
/// surface it's drawn on (page, hover, bar), and warnings are a brick red rather than a pure one.
fn palette(dark: bool, accent: Accent) -> Ink {
    if dark {
        Ink {
            bg: Color32::from_rgb(0x18, 0x17, 0x15),
            text: Color32::from_rgb(0xe4, 0xe0, 0xd8),
            muted: Color32::from_rgb(0x92, 0x8d, 0x84),
            faint: Color32::from_rgb(0x3b, 0x38, 0x33),
            line: Color32::from_rgb(0x73, 0x6e, 0x65),
            hover: Color32::from_rgb(0x22, 0x1f, 0x1c),
            row_hover: Color32::from_rgb(0x2a, 0x27, 0x23),
            selected: Color32::from_rgb(0x18, 0x17, 0x15).lerp_to_gamma(accent.colour(true), accent.tint()),
            accent: accent.colour(true),
            warn: Color32::from_rgb(0xe4, 0x82, 0x70),
            bar: Color32::from_rgb(0x1f, 0x1d, 0x1a),
            chip: Color32::from_rgb(0x35, 0x32, 0x2c),
        }
    } else {
        Ink {
            bg: Color32::from_rgb(0xfa, 0xf8, 0xf2),
            text: Color32::from_rgb(0x33, 0x30, 0x2b),
            muted: Color32::from_rgb(0x6b, 0x67, 0x5f),
            faint: Color32::from_rgb(0xe0, 0xdb, 0xcf),
            line: Color32::from_rgb(0x8f, 0x89, 0x7e),
            hover: Color32::from_rgb(0xf2, 0xef, 0xe7),
            row_hover: Color32::from_rgb(0xec, 0xe8, 0xde),
            selected: Color32::from_rgb(0xfa, 0xf8, 0xf2).lerp_to_gamma(accent.colour(false), accent.tint()),
            accent: accent.colour(false),
            warn: Color32::from_rgb(0xa5, 0x40, 0x2f),
            bar: Color32::from_rgb(0xf0, 0xec, 0xe2),
            chip: Color32::from_rgb(0xe3, 0xde, 0xd1),
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

/// egui puts a font's line gap below each line, so text sits that much above the middle of its
/// row, and above the icons beside it. Windows' Palatino Linotype has a gap of a third of its
/// size; without it, text is centred as on macOS. Only plain font files: collections are left
/// as they are.
fn without_line_gap(mut font: Vec<u8>) -> Vec<u8> {
    let at = |b: &[u8], i: usize, n: usize| b.get(i..i + n).map(|s| s.iter().fold(0usize, |v, &x| v << 8 | x as usize));
    if !matches!(font.get(..4), Some([0, 1, 0, 0] | b"true" | b"OTTO")) {
        return font;
    }
    let tables = at(&font, 4, 2).unwrap_or(0);
    let hhea = (0..tables).map(|t| 12 + 16 * t).find(|&r| font.get(r..r + 4) == Some(b"hhea"));
    if let Some(gap) = hhea.and_then(|r| at(&font, r + 8, 4)).map(|offset| offset + 8)
        && let Some(b) = font.get_mut(gap..gap + 2)
    {
        b.fill(0);
    }
    font
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut load = |list: &[(&str, u32)], first_only: bool| -> Vec<String> {
        let mut names = Vec::new();
        for (path, index) in list {
            if let Ok(bytes) = std::fs::read(path) {
                let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
                fonts.font_data.insert(name.clone(), Arc::new(FontData::from_blob(Arc::new(without_line_gap(bytes)), *index)));
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
            // The 8-point grid (`layout`): gaps of 8, controls a row of 24 tall, insets of 16.
            let s = &mut style.spacing;
            s.item_spacing = vec2(S1, S1);
            s.button_padding = vec2(S1, 0.0);
            s.interact_size = vec2(S5, ROW);
            s.icon_width = ICON;
            s.icon_width_inner = S1;
            s.icon_spacing = S1;
            s.indent = S2;
            s.menu_margin = egui::Margin::same(S1 as i8);
            s.window_margin = egui::Margin::same(S2 as i8);
            let v = &mut style.visuals;
            let ink = palette(theme == Theme::Dark, Accent::default());
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
                w.bg_stroke = Stroke::new(1.0, ink.line);
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
    egui::Frame::new().fill(ink.hover).stroke(Stroke::new(1.0, ink.faint)).corner_radius(8.0).inner_margin(egui::Margin::same(S2 as i8))
}

/// A dialog over the app: the paper, with a grid's breadth of room inside.
pub fn dialog_frame(ui: &Ui) -> egui::Frame {
    let ink = ink(ui);
    egui::Frame::popup(ui.style()).fill(ink.bg).inner_margin(egui::Margin::same(S3 as i8))
}

/// Outlined in the warning colour: something the user should know before it surprises them.
pub fn warning_frame(ui: &Ui) -> egui::Frame {
    let ink = ink(ui);
    egui::Frame::new().stroke(Stroke::new(1.0, ink.warn)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(S2 as i8, S1 as i8))
}

#[cfg(test)]
mod tests {
    #[test]
    fn line_gap_is_dropped_from_plain_fonts_only() {
        // A font file with one table, `hhea`: ascender 1499, descender -582, line gap 682.
        let mut font = vec![0, 1, 0, 0, 0, 1, 0, 16, 0, 0, 0, 0];
        font.extend(b"hhea");
        font.extend([0, 0, 0, 0, 0, 0, 0, 28, 0, 0, 0, 36]);
        font.extend([0, 1, 0, 0, 0x05, 0xdb, 0xfd, 0xba, 0x02, 0xaa]);
        font.resize(28 + 36, 0);
        let fixed = super::without_line_gap(font.clone());
        assert_eq!(fixed[28 + 4..28 + 10], [0x05, 0xdb, 0xfd, 0xba, 0, 0], "ascender and descender kept, gap gone");
        let mut collection = font;
        collection[..4].copy_from_slice(b"ttcf");
        assert_eq!(super::without_line_gap(collection.clone()), collection);
        assert!(super::without_line_gap(vec![0, 1, 0, 0, 0, 9]).len() == 6, "a truncated file is left alone");
    }
}
