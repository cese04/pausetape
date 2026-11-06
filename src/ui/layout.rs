//! The layout grid. Every gap, padding and margin in the app is a multiple of 8 points, taken
//! from here, and every control sits on a row that's a whole number of them tall:
//!
//! - `S1` (8): between related things — an icon and its words, controls in a row, lines of a list.
//! - `S2` (16): inside panels and cards; between groups of controls.
//! - `S3` (24): between sections; inside dialogs.
//! - `S4`–`S6` (32–48): the page's own margins, and the breaks between its parts.
//!
//! Rows of controls or songs go left to right with everything centred on one line (`row`), so
//! icons and words never sit at different heights.

use egui::{Align, InnerResponse, Layout, Margin, Ui, vec2};

pub const S1: f32 = 8.0;
pub const S2: f32 = 16.0;
pub const S3: f32 = 24.0;
pub const S4: f32 = 32.0;
pub const S5: f32 = 40.0;
pub const S6: f32 = 48.0;

/// The height of a row of controls, and of the small ones in it (links, chips, fields).
pub const ROW: f32 = 24.0;
/// The height of buttons, tabs and other controls that lead.
pub const CONTROL: f32 = 32.0;
/// Icons drawn beside words.
pub const ICON: f32 = 16.0;

/// The type scale, each a fifth or so above the last, in points. ET Book has a small eye for its
/// size, so nothing read is under 13.
pub mod font {
    /// Section labels, in capitals.
    pub const CAPS: f32 = 12.0;
    /// Lesser facts: paths, notes, badges.
    pub const META: f32 = 13.0;
    /// The interface: links, help, compact rows.
    pub const UI: f32 = 14.0;
    /// Rows and reading: songs, albums, lyrics.
    pub const BODY: f32 = 16.0;
    /// A row's or a dialog's title, the song playing.
    pub const TITLE: f32 = 20.0;
    /// A page's title.
    pub const HEADING: f32 = 24.0;
    /// The album in the details panel.
    pub const DISPLAY: f32 = 28.0;
}

/// A frame's margins: `x` left and right, `y` top and bottom (both on the grid).
pub fn margin(x: f32, y: f32) -> Margin {
    Margin::symmetric(x as i8, y as i8)
}

/// A row of controls or text: left to right, each item centred on the row's middle line, at
/// least one `ROW` tall. (A bare `with_layout` would centre the row in all the space left below.)
pub fn row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    ui.allocate_ui_with_layout(vec2(ui.available_size_before_wrap().x, ROW), Layout::left_to_right(Align::Center), add)
}

/// `row`, wrapping onto further rows when it runs out of width.
pub fn row_wrapped<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    ui.allocate_ui_with_layout(vec2(ui.available_size_before_wrap().x, ROW), Layout::left_to_right(Align::Center).with_main_wrap(true), add)
}
