//! An in-app folder browser: shortcuts, a clickable breadcrumb, and the subfolders of the
//! current folder with how many audio files each holds directly, which shows at a glance
//! whether a tree is already organised. On Windows, "Computer" above the drives lists them.

use std::fs;
use std::path::{Path, PathBuf};

use egui::{RichText, ScrollArea, Ui};

use super::text;
use super::theme::ink;
use super::widgets::{self, caps, link, muted};
use super::layout::{S1, row_wrapped};

const AUDIO: &[&str] = &["flac", "mp3", "wav", "m4a", "aac", "ogg", "opus", "ape", "wv", "aiff", "aif", "wma"];
/// Past this many subfolders, skip counting their audio files to keep browsing instant.
const COUNT_LIMIT: usize = 400;
/// Entries examined per subfolder when counting songs inside it; beyond this we show "n+".
const COUNT_BUDGET: usize = 3000;

struct Listing {
    dir: PathBuf,
    folders: Vec<Entry>,
    audio: usize,
    other: usize,
    error: Option<String>,
}

/// A folder, or on Windows a drive, to open.
struct Entry {
    name: String,
    path: PathBuf,
    /// Songs inside it, and whether counting stopped early.
    songs: Option<(usize, bool)>,
    /// Said at the right instead of the songs: a drive's free space.
    note: Option<String>,
}

/// Where "Computer" is: the drives' list on Windows, which has no folder of its own.
fn is_computer(dir: &Path) -> bool {
    cfg!(windows) && dir.as_os_str().is_empty()
}

/// Where the "Drives" shortcut goes.
fn drives_place() -> PathBuf {
    if cfg!(windows) {
        PathBuf::new()
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Volumes")
    } else {
        PathBuf::from("/media")
    }
}

pub struct Navigator {
    dir: PathBuf,
    listing: Option<Listing>,
}

fn is_audio(p: &Path) -> bool {
    p.extension().is_some_and(|e| AUDIO.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

fn hidden(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$')
}

/// Songs anywhere inside `dir` (a few levels deep), and whether the count stopped early.
fn audio_files_in(dir: &Path) -> (usize, bool) {
    let mut seen = 0;
    let mut songs = 0;
    for e in walkdir::WalkDir::new(dir).max_depth(4).into_iter().filter_entry(|e| !hidden(&e.file_name().to_string_lossy())).flatten() {
        seen += 1;
        if seen > COUNT_BUDGET {
            return (songs, true);
        }
        if e.file_type().is_file() && is_audio(e.path()) {
            songs += 1;
        }
    }
    (songs, false)
}

fn list(dir: &Path) -> Listing {
    let mut l = Listing { dir: dir.to_path_buf(), folders: Vec::new(), audio: 0, other: 0, error: None };
    #[cfg(windows)]
    if is_computer(dir) {
        l.folders = crate::card::drives().into_iter().map(|v| Entry { note: v.info.as_ref().map(describe_space), name: v.name, path: v.path, songs: None }).collect();
        return l;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            l.error = Some(e.to_string());
            return l;
        }
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if hidden(&name) {
            continue;
        }
        let path = e.path();
        if path.is_dir() {
            l.folders.push(Entry { name, path, songs: None, note: None });
        } else if is_audio(&path) {
            l.audio += 1;
        } else {
            l.other += 1;
        }
    }
    l.folders.sort_by_key(|f| f.name.to_lowercase());
    if l.folders.len() <= COUNT_LIMIT {
        for f in &mut l.folders {
            f.songs = Some(audio_files_in(&f.path));
        }
    }
    l
}

impl Navigator {
    pub fn new(start: Option<&Path>) -> Self {
        let dir = start
            .filter(|p| p.is_dir())
            .map(Path::to_path_buf)
            .or_else(std::env::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        Navigator { dir, listing: None }
    }

    /// Opened on the drives: /Volumes, /media, or Windows' "Computer".
    pub fn on_drives() -> Self {
        let dir = drives_place();
        if is_computer(&dir) || dir.is_dir() { Navigator { dir, listing: None } } else { Navigator::new(None) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether the current place is a folder that can be chosen (not Windows' list of drives).
    pub fn can_choose(&self) -> bool {
        !is_computer(&self.dir)
    }

    /// Display name of the current folder.
    pub fn name(&self) -> String {
        if is_computer(&self.dir) {
            return "Computer".into();
        }
        self.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.dir.display().to_string())
    }

    /// One level up; from a drive's top folder on Windows, the list of drives.
    fn up(&self) -> Option<PathBuf> {
        if is_computer(&self.dir) {
            return None;
        }
        self.dir.parent().map(Path::to_path_buf).or_else(|| cfg!(windows).then(PathBuf::new))
    }

    fn go(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.listing = None;
    }

    /// Draws the browser: shortcuts, breadcrumb, then one clickable row per subfolder.
    pub fn show(&mut self, ui: &mut Ui) {
        let ink = ink(ui);
        if self.listing.as_ref().is_none_or(|l| l.dir != self.dir) {
            self.listing = Some(list(&self.dir));
        }
        let mut go = None;

        row_wrapped(ui, |ui| {
            let mut places: Vec<(&str, PathBuf)> = Vec::new();
            if let Some(h) = std::env::home_dir() {
                places.push(("Home", h.clone()));
                places.push(("Music", h.join("Music")));
                places.push(("Downloads", h.join("Downloads")));
            }
            places.push(("Drives", drives_place()));
            for (label, p) in places {
                if (is_computer(&p) || p.is_dir()) && widgets::button(ui, label).clicked() {
                    go = Some(p);
                }
            }
            let picker = if is_computer(&self.dir) { rfd::FileDialog::new() } else { rfd::FileDialog::new().set_directory(&self.dir) };
            if link(ui, "System picker…").clicked()
                && let Some(p) = picker.pick_folder()
            {
                go = Some(p);
            }
        });
        ui.add_space(S1);

        // Breadcrumb: every ancestor is a link; on Windows, "Computer" comes first.
        row_wrapped(ui, |ui| {
            caps(ui, "in");
            if is_computer(&self.dir) {
                ui.label(RichText::new("Computer").strong().color(ink.text));
                return;
            }
            if cfg!(windows) {
                if link(ui, "Computer").clicked() {
                    go = Some(PathBuf::new());
                }
                muted(ui, "›");
            }
            let ancestors: Vec<&Path> = self.dir.ancestors().collect();
            for (i, a) in ancestors.iter().rev().enumerate() {
                // A top folder has no name: "/" is the computer; on Windows, "D:\" is "D:".
                let top = || if cfg!(windows) { a.display().to_string().trim_end_matches('\\').to_string() } else { "Computer".into() };
                let name = a.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(top);
                if i + 1 == ancestors.len() {
                    ui.label(RichText::new(name).strong().color(ink.text));
                } else {
                    if link(ui, name).clicked() {
                        go = Some(a.to_path_buf());
                    }
                    muted(ui, "›");
                }
            }
        });

        let l = self.listing.as_ref().unwrap();
        let here = match (l.audio, l.folders.len()) {
            (_, n) if is_computer(&l.dir) => format!("{} on this computer.", text::plural(n, "drive")),
            (0, 0) => "This folder is empty.".to_string(),
            (0, n) => format!("{n} folders, no songs directly in here."),
            (a, n) => format!("{a} songs directly in here, {n} folders."),
        };
        muted(ui, here);
        if let Some(e) = &l.error {
            ui.label(RichText::new(e).color(ink.warn).size(12.5));
        }
        ui.add_space(S1);
        let row_h = 26.0;
        ScrollArea::vertical().id_salt("nav").max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            let row = |ui: &mut Ui, label: String, right: Option<String>, folder: bool| -> bool {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
                super::widgets::name(ui, &resp, egui::Role::Button, &label);
                super::widgets::focus_ring(ui, &resp, rect, 4.0);
                if resp.hovered() || resp.has_focus() {
                    ui.painter().rect_filled(rect, 4.0, ink.faint);
                }
                let y = rect.center().y;
                let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0, y), egui::vec2(14.0, 14.0));
                if folder {
                    super::icons::paint(ui.painter(), icon, super::icons::Icon::Folder, ink.muted);
                } else {
                    ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, "↑", egui::FontId::proportional(15.0), ink.muted);
                }
                ui.painter().text(egui::pos2(rect.left() + 32.0, y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), ink.text);
                if let Some(text) = right {
                    ui.painter().text(egui::pos2(rect.right() - 8.0, y), egui::Align2::RIGHT_CENTER, text, egui::FontId::proportional(12.5), ink.muted);
                }
                resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
            };
            if let Some(up) = self.up()
                && row(ui, "back up".into(), None, false)
            {
                go = Some(up);
            }
            for f in &l.folders {
                let songs = f.songs.filter(|(n, _)| *n > 0).map(|(n, more)| format!("{n}{} songs", if more { "+" } else { "" }));
                if row(ui, text::shorten(&f.name, 70), f.note.clone().or(songs), true) {
                    go = Some(f.path.clone());
                }
            }
        });
        if let Some(d) = go {
            self.go(d);
        }
    }
}

/// "12.3 GB free of 128 GB · exfat"
pub fn describe_space(info: &crate::card::SpaceInfo) -> String {
    let mut s = format!("{} free of {}", text::bytes(info.free), text::bytes(info.total));
    if let Some(fs) = info.fs_name() {
        s.push_str(&format!(" · {fs}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::Navigator;

    #[test]
    fn up_from_a_drive_is_the_computer() {
        let nav = |p: &str| Navigator { dir: PathBuf::from(p), listing: None };
        if cfg!(windows) {
            assert_eq!(nav(r"D:\Music").up(), Some(PathBuf::from(r"D:\")));
            assert_eq!(nav(r"D:\").up(), Some(PathBuf::new()), "a drive's top folder goes up to the drives");
            let computer = nav("");
            assert_eq!((computer.up(), computer.can_choose(), computer.name().as_str()), (None, false, "Computer"));
            assert!(nav(r"D:\").can_choose());
        } else {
            assert_eq!(nav("/").up(), None);
            assert_eq!(nav("/Volumes").up().as_deref(), Some(Path::new("/")));
            assert!(nav("/").can_choose());
        }
    }
}
