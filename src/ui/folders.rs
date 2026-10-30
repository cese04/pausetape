//! Folders tab: two steps, each with one clear choice. Choosing happens in a dialog so the tab
//! itself only ever shows what is set and how to change it.

use std::path::{Path, PathBuf};

use egui::{RichText, ScrollArea, Ui};

use super::navigator::Navigator;
use super::text::{self, plural};
use super::theme::{ink, panel_frame};
use super::icons::Icon;
use super::widgets::{StrikeIf, button, button_with, caps, icon_link, link, muted, primary_button, primary_with};
use super::ROW_H;
use crate::app::{App, Tab};
use crate::card::{AlbumOrder, SpaceInfo, Volume};

/// The open folder dialog, if any.
pub enum Picker {
    Source(Navigator),
    Card { browse: Option<Navigator>, show_all: bool },
}

pub fn show(app: &mut App, ui: &mut Ui) {
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.set_max_width(ui.available_width().min(680.0));
        muted(ui, "Two things to set up once: where your music is, and which SD card it goes to.");
        ui.add_space(12.0);
        source_step(app, ui);
        ui.add_space(14.0);
        card_step(app, ui);
        ui.add_space(18.0);
        if app.saved.source.is_some() && app.write().dest.is_some() {
            ui.horizontal(|ui| {
                muted(ui, "All set. Next, choose what goes on the card.");
                if primary_with(ui, Icon::Card, "Go to the card").clicked() {
                    app.tab = Tab::Card;
                }
            });
            ui.add_space(18.0);
        }
        egui::CollapsingHeader::new(RichText::new("Advanced: reorganise the music folder itself").color(ink(ui).muted))
            .default_open(app.tidy_plan.is_some())
            .show(ui, |ui| tidy(app, ui));
    });
}

fn step_title(ui: &mut Ui, n: &str, title: &str) {
    let ink = ink(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(n).size(22.0).color(ink.muted));
        ui.label(RichText::new(title).size(19.0).color(ink.text));
    });
}

fn folder_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

fn source_step(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    panel_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        step_title(ui, "1", "Your music");
        ui.add_space(6.0);
        match app.saved.source.clone() {
            None => {
                muted(ui, "The folder on this computer that holds your music. It can be messy; NeatLibrary only reads it.");
            }
            Some(src) => {
                ui.label(RichText::new(folder_name(&src)).size(20.0).strong());
                ui.add(egui::Label::new(RichText::new(src.display().to_string()).size(12.0).color(ink.muted)).truncate());
                ui.add_space(4.0);
                if app.busy.as_ref().is_some_and(|b| b.label == "reading tags") {
                    muted(ui, "reading your music…");
                } else {
                    let artists = app.index.artists.len();
                    let albums: usize = app.index.artists.iter().map(|a| a.albums.len()).sum();
                    ui.label(format!("{} · {} · {}", plural(app.tracks.len(), "song"), plural(artists, "artist"), plural(albums, "album")));
                }
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = if app.saved.source.is_some() { "Change folder…" } else { "Choose folder…" };
            if button_with(ui, Icon::Folder, label).clicked() {
                app.picker = Some(Picker::Source(Navigator::new(app.saved.source.as_deref())));
            }
            if app.saved.source.is_some() && app.busy.is_none() && icon_link(ui, Icon::Rescan, "read again").on_hover_text("pick up files added or changed since").clicked() {
                app.scan();
            }
        });
    });
}

/// The mounted volume a path lives on, if we know it.
fn volume_of<'a>(vols: &'a [Volume], p: &Path) -> Option<&'a Volume> {
    vols.iter().filter(|v| p.starts_with(&v.path)).max_by_key(|v| v.path.as_os_str().len())
}

fn space_bar(ui: &mut Ui, info: &SpaceInfo, width: f32) {
    let ink = ink(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 8.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 4.0, ink.faint);
    let used = 1.0 - info.free as f32 / info.total.max(1) as f32;
    let mut filled = rect;
    filled.set_width(rect.width() * used.clamp(0.0, 1.0));
    ui.painter().rect_filled(filled, 4.0, ink.muted);
}

fn card_step(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    let dest = app.write().dest.clone();
    let write_name = app.write().name.clone();
    panel_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        step_title(ui, "2", "Your SD card");
        if app.saved.writes.len() > 1 {
            muted(ui, format!("for the write “{write_name}”"));
        }
        ui.add_space(6.0);
        match &dest {
            None => {
                muted(ui, "Put the Echo Mini's card in a reader (or connect the player as a drive), then choose it.");
            }
            Some(d) => {
                let vol = volume_of(&app.volumes, d);
                let title = match vol {
                    Some(v) if v.path == *d => v.name.clone(),
                    Some(v) => format!("{} › {}", v.name, d.strip_prefix(&v.path).map(|r| r.display().to_string()).unwrap_or_default()),
                    None => folder_name(d),
                };
                ui.label(RichText::new(title).size(20.0).strong());
                match crate::card::space(d) {
                    _ if !d.exists() => {
                        ui.label(RichText::new("Not connected right now. Insert the card to write to it.").color(ink.accent));
                    }
                    Some(info) => {
                        space_bar(ui, &info, 320.0);
                        let fs = info.fs_name().map(|f| format!(" · {f}")).unwrap_or_default();
                        muted(ui, format!("{} free of {}{fs}", text::bytes(info.free), text::bytes(info.total)));
                        if info.is_fat() == Some(false) {
                            ui.label(RichText::new("This drive isn't formatted FAT32/exFAT, so it's probably not the player's card.").color(ink.accent));
                        }
                    }
                    // Connected, but this OS doesn't report space yet (Windows, for now).
                    None => {
                        muted(ui, "Connected. Free space isn't shown on this system yet.");
                    }
                }
            }
        }
        ui.add_space(8.0);
        let label = if dest.is_some() { "Change card…" } else { "Choose card…" };
        if button_with(ui, Icon::Card, label).clicked() {
            app.volumes = crate::card::volumes();
            app.picker = Some(Picker::Card { browse: None, show_all: false });
        }
    });
}

/// Dialogs for choosing the music folder or the card; drawn over whichever tab is open.
pub fn pickers(app: &mut App, ui: &mut Ui) {
    let Some(mut picker) = app.picker.take() else { return };
    let ink = ink(ui);
    let mut close = false;
    let mut chosen_source: Option<PathBuf> = None;
    let mut chosen_dest: Option<PathBuf> = None;
    let mut refresh = false;
    let frame = egui::Frame::popup(ui.style()).fill(ink.bg).inner_margin(egui::Margin::same(22));
    let modal = egui::Modal::new(egui::Id::unique("picker")).frame(frame).show(ui.ctx(), |ui| {
        // Fit small windows: the dialog never gets wider than the window allows.
        ui.set_width(560.0f32.min(ui.ctx().content_rect().width() - 80.0));
        match &mut picker {
            Picker::Source(nav) => {
                ui.label(RichText::new("Choose your music folder").size(20.0));
                muted(ui, "Open folders until you're in the one that holds all your music, then use it.");
                ui.add_space(10.0);
                nav.show(ui);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, format!("Use “{}”", nav.name())).clicked() {
                        chosen_source = Some(nav.dir().to_path_buf());
                    }
                    if button(ui, "Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Picker::Card { browse: Some(nav), .. } => {
                ui.label(RichText::new("Choose a folder on the card").size(20.0));
                muted(ui, "Music will go inside this folder instead of at the top of the card.");
                ui.add_space(10.0);
                nav.show(ui);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, format!("Use “{}”", nav.name())).clicked() {
                        chosen_dest = Some(nav.dir().to_path_buf());
                    }
                    if button(ui, "Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Picker::Card { browse, show_all } => {
                ui.label(RichText::new("Choose the SD card").size(20.0));
                ui.add_space(10.0);
                let (cards, others): (Vec<&Volume>, Vec<&Volume>) =
                    app.volumes.iter().partition(|v| v.info.and_then(|i| i.is_fat()).unwrap_or(true));
                if cards.is_empty() {
                    muted(ui, "No SD cards found. Insert the card (or connect the player as a drive) and refresh.");
                }
                let list: Vec<&Volume> = if *show_all { cards.iter().chain(others.iter()).copied().collect() } else { cards };
                for v in list {
                    panel_frame(ui).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&v.name).size(17.0).strong());
                                if let Some(info) = &v.info {
                                    space_bar(ui, info, 220.0);
                                    let fs = info.fs_name().map(|f| format!(" · {f}")).unwrap_or_default();
                                    muted(ui, format!("{} free of {}{fs}", text::bytes(info.free), text::bytes(info.total)));
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if primary_button(ui, "Use this card").clicked() {
                                    chosen_dest = Some(v.path.clone());
                                }
                            });
                        });
                    });
                    ui.add_space(6.0);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if button(ui, "Refresh").clicked() {
                        refresh = true;
                    }
                    if !others.is_empty() {
                        let label = if *show_all { "Hide other drives".to_string() } else { format!("Show other drives ({})", others.len()) };
                        if link(ui, label).clicked() {
                            *show_all = !*show_all;
                        }
                    }
                    if link(ui, "Use a folder on a card…").clicked() {
                        let start = app.write().dest.clone().unwrap_or_else(|| PathBuf::from("/Volumes"));
                        *browse = Some(Navigator::new(Some(&start)));
                    }
                });
                ui.add_space(8.0);
                if button(ui, "Cancel").clicked() {
                    close = true;
                }
            }
        }
    });
    if modal.should_close() {
        close = true;
    }
    if refresh {
        app.volumes = crate::card::volumes();
    }
    if let Some(dir) = chosen_source {
        app.set_source(dir);
        close = true;
    }
    if let Some(dir) = chosen_dest {
        if app.saved.source.as_ref().is_some_and(|s| dir.starts_with(s) || s.starts_with(&dir)) {
            app.notice("the card folder can't be inside your music folder (or the other way round)");
        } else {
            app.write_mut().dest = Some(dir);
            close = true;
        }
    }
    if !close {
        app.picker = Some(picker);
    }
}

fn tidy(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    muted(
        ui,
        "You don't need this to use the card: writes read your music wherever it is. Use it only if you want the folder on this computer renamed into Artist / Album / Track as well. It moves files, and can be undone.",
    );
    if app.saved.source.is_none() || app.tracks.is_empty() {
        return;
    }
    muted(ui, format!("{} of {} songs are already where this would put them.", app.in_place, app.tracks.len()));
    let mut changed = false;
    ui.horizontal(|ui| {
        caps(ui, "album folders");
        let t = &mut app.saved.tidy;
        changed |= ui.radio_value(&mut t.album_order, AlbumOrder::Year, "1997 - Album").changed();
        changed |= ui.radio_value(&mut t.album_order, AlbumOrder::Title, "Album").changed();
        changed |= ui.checkbox(&mut t.write_tags, "also save corrected tags").changed();
    });
    if changed {
        app.tidy_plan = None;
    }
    ui.horizontal(|ui| {
        if button_with(ui, Icon::Compare, "Preview changes").clicked() {
            app.preview_tidy();
        }
        if app.tidy_plan.is_some() && app.busy.is_none() && button_with(ui, Icon::Folder, "Reorganise now").clicked() {
            app.run_tidy();
        }
        if crate::card::latest_journal().is_some() && icon_link(ui, Icon::Back, "undo last reorganise").clicked() {
            app.undo();
        }
    });
    let Some(plan) = &app.tidy_plan else { return };
    let src = app.saved.source.clone().unwrap_or_default();
    let moving = plan.iter().filter(|p| src.join(&p.rel) != app.tracks[p.track].path).count();
    ui.label(RichText::new(format!("{moving} of {} files would move. RECHO's resume position may point at a moved file afterwards.", plan.len())).color(ink.accent).size(13.0));
    plan_list(ui, plan, 320.0);
}

/// Every planned path, folders muted and file names in ink; left-out files struck through.
pub fn plan_list(ui: &mut Ui, plan: &[crate::card::PlanItem], max_height: f32) {
    let ink = ink(ui);
    ScrollArea::vertical().id_salt("plan").max_height(max_height).auto_shrink([false, true]).show_rows(ui, ROW_H - 4.0, plan.len(), |ui, range| {
        for p in &plan[range] {
            let rel = p.rel.to_string_lossy();
            let (dir, file) = rel.rsplit_once(std::path::MAIN_SEPARATOR).unwrap_or(("", &rel));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(RichText::new(format!("{dir}/")).color(ink.muted).size(13.0));
                ui.label(RichText::new(file).size(13.0).color(if p.skip.is_some() { ink.muted } else { ink.text }).strikethrough_if(p.skip.is_some()));
                if let Some(why) = p.skip {
                    ui.label(RichText::new(format!("   {why}")).size(12.0).color(ink.accent));
                }
            });
        }
    });
}
