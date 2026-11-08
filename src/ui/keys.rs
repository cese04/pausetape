//! Keyboard shortcuts: the keys, when they're listened for, and the list Settings shows, written
//! as each system writes them (⌘ on macOS, Ctrl elsewhere). The library's own keys (arrows,
//! Enter, ⌘A, ⌘I, ⌘D) act on what its views show, so `library` and `albums` read them.

use egui::gui_zoom::kb_shortcuts::{ZOOM_IN, ZOOM_OUT, ZOOM_RESET};
use egui::{Key, KeyboardShortcut, Modifiers, Ui};

use crate::app::{App, Tab};

pub const SEARCH: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
pub const SELECT_ALL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
pub const EDIT_TAGS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
pub const CARD: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
pub const SYNC: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
pub const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);

/// A click with the command key, and the command key with Return, named as the system names them.
pub const COMMAND_CLICK: &str = if cfg!(target_os = "macos") { "⌘-click" } else { "Ctrl+click" };
pub const COMMAND_RETURN: &str = if cfg!(target_os = "macos") { "⌘-Return" } else { "Ctrl+Enter" };

/// Every shortcut and what it does, for Settings.
pub fn list(ctx: &egui::Context) -> Vec<(String, &'static str)> {
    let f = |s: &KeyboardShortcut| ctx.format_shortcut(s);
    vec![
        (f(&SEARCH), "search the library (on the card's view: its albums)"),
        ("↑ ↓".into(), "select the song or album above or below; with Shift, add it to the selection"),
        ("Enter".into(), "play what's selected"),
        (f(&SELECT_ALL), "select everything shown"),
        (f(&EDIT_TAGS), "edit the tags of what's selected"),
        (f(&CARD), "send what's selected to the card, or take it off if it's all there"),
        (f(&SYNC), "Sync the card"),
        (f(&UNDO), "undo the latest change to playlists or to what goes on the card"),
        ("Esc".into(), "clear the selection or the search; close a dialog"),
        ("Space".into(), "play or pause"),
        ("← →".into(), "back or forward 10 seconds"),
        ("N  P".into(), "next or previous song"),
        ("Tab".into(), "move between controls; Space or Enter presses the one with the ring"),
        (format!("{}  {}  {}", f(&ZOOM_IN), f(&ZOOM_OUT), f(&ZOOM_RESET)), "larger or smaller text, or as designed (also in Settings)"),
    ]
}

/// Keys are for typing while a text field has them, and for the dialog while one is open.
pub fn taken(app: &App, ui: &Ui) -> bool {
    ui.ctx().egui_wants_keyboard_input() || app.dialog_open()
}

/// The arrows and Enter also belong to a control focused with Tab (they move between controls
/// and press it).
pub fn navigation_free(app: &App, ui: &Ui) -> bool {
    !taken(app, ui) && ui.ctx().memory(|m| m.focused().is_none())
}

pub fn pressed(ui: &Ui, shortcut: &KeyboardShortcut) -> bool {
    ui.input_mut(|i| i.consume_shortcut(shortcut))
}

/// Plain arrow or Enter presses, without ⌘ (⌘↑ and the like are left to the system).
pub fn nav_key(ui: &Ui, key: Key) -> Option<bool> {
    ui.input(|i| (i.key_pressed(key) && !i.modifiers.command && !i.modifiers.alt).then_some(i.modifiers.shift))
}

/// Shortcuts that work from any tab: search, Sync, undo.
pub fn global(app: &mut App, ui: &Ui) {
    if taken(app, ui) {
        return;
    }
    if pressed(ui, &UNDO) {
        app.undo_last();
    }
    if pressed(ui, &SEARCH) {
        if !matches!(app.tab, Tab::Library | Tab::Card) {
            app.tab = Tab::Library;
        }
        app.focus_search = true;
    }
    if pressed(ui, &SYNC) {
        let connected = app.card_connected();
        if let Some(b) = &app.busy {
            app.notice(format!("Sync waits until {} is done", b.label));
        } else if !connected {
            app.notice(format!("the card for “{}” isn't connected", app.write().name));
        } else {
            app.run_write();
        }
    }
}
