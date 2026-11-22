//! Dragging music: songs or albums from the library onto a playlist in the sidebar, and a
//! playlist's songs into a new order. What's held follows the pointer, in words.

use egui::{FontId, Response, Stroke};

use super::layout::{S1, font};
use super::text::plural;
use super::theme::ink;

/// Songs being dragged (track indices).
pub struct Songs(pub Vec<usize>);

/// A playlist's song being moved: (playlist, its position).
pub struct PlaylistSong(pub usize, pub usize);

/// Make `resp` a drag source for these songs (worked out only when the drag starts).
pub fn songs_from(resp: &Response, songs: impl FnOnce() -> Vec<usize>) {
    if resp.drag_started() {
        resp.dnd_set_drag_payload(Songs(songs()));
    }
}

/// A place to drop songs: outlined in the accent while they're held over it; the songs when
/// they're let go there.
pub fn songs_to(ui: &egui::Ui, resp: &Response, rect: egui::Rect) -> Option<Vec<usize>> {
    if resp.dnd_hover_payload::<Songs>().is_some() {
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.5, ink(ui).accent), egui::StrokeKind::Inside);
    }
    resp.dnd_release_payload::<Songs>().map(|s| s.0.clone())
}

/// What's held, beside the pointer, while it's dragged: in ink on the page's colour, reversed.
pub fn ghost(ui: &egui::Ui) {
    let ctx = ui.ctx();
    let Some(pos) = ctx.pointer_latest_pos() else { return };
    let words = if let Some(s) = egui::DragAndDrop::payload::<Songs>(ctx) {
        format!("{} · drop on a playlist", plural(s.0.len(), "song"))
    } else if egui::DragAndDrop::has_payload_of_type::<PlaylistSong>(ctx) {
        "moving the song".to_string()
    } else {
        return;
    };
    let ink = ink(ui);
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag ghost")));
    let g = p.layout_no_wrap(words, FontId::proportional(font::UI), ink.bg);
    let rect = egui::Rect::from_min_size(pos + egui::vec2(14.0, 10.0), g.size() + egui::vec2(2.0 * S1, S1));
    p.rect_filled(rect, 4.0, ink.text);
    p.galley(rect.min + egui::vec2(S1, S1 / 2.0), g, ink.bg);
}
