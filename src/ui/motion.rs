//! Movement (§4.7 of the design notes): only to show what changed, quick, never bouncing; none
//! at all when Settings or the system ask for less, and the reels stay still.

use egui::{Context, Id};

/// Hover and focus: a colour coming and going.
pub const HOVER: f32 = 0.12;
/// A part of the window changing size: the player folding or unfolding.
pub const RESIZE: f32 = 0.25;
/// A figure going to its new value: the card's room after a Sync.
pub const SETTLE: f32 = 0.3;

/// egui's own (panels, sections opening, scrolling to a row), as it comes.
const EGUI_DEFAULT: f32 = 1.0 / 12.0;

fn id() -> Id {
    Id::unique("reduce motion")
}

/// Once a frame, from Settings and the system.
pub fn set_reduced(ctx: &Context, reduced: bool) {
    if ctx.data(|d| d.get_temp::<bool>(id())) == Some(reduced) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id(), reduced));
    ctx.all_styles_mut(|s| s.animation_time = if reduced { 0.0 } else { EGUI_DEFAULT });
}

pub fn reduced(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(id())).unwrap_or(false)
}

/// How long a change of this kind takes: `secs`, or none when motion is reduced.
pub fn time(ctx: &Context, secs: f32) -> f32 {
    if reduced(ctx) { 0.0 } else { secs }
}
